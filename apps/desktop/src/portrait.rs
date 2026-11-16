//! The signed-in person's picture: the one their Google account has, whose
//! address the server gives with the session (`user.image`).
//!
//! It is the one thing the app draws that it fetches, so what may be fetched
//! is decided here and nowhere else: an https address on Google's picture
//! host, asked for with no session, cookie or referrer and no redirect
//! followed (`fetch::bytes`), and at most [`LIMIT`] bytes of it. The bytes
//! are decoded before anything is drawn and kept in memory and in
//! `<data directory>/portraits`, so the next launch draws the picture at once
//! and without the network. Whenever there is no picture (none on the
//! account, an address that is not Google's, no answer, bytes that are not an
//! image) the caller draws the person's initial, exactly as before.
use gpui::*;
use meeting_notes_api_types::AuthSessionDTO;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// The largest picture read, from the network or from the disk.
const LIMIT: usize = 1024 * 1024;
/// The edge asked of Google, in pixels: the largest circle is 32 points, and
/// this stays sharp at twice that and more.
const EDGE: u32 = 128;
/// A picture that did not load is asked for again after this long.
const RETRY: Duration = Duration::from_secs(10 * 60);
/// A saved picture older than this is fetched again (and drawn meanwhile).
const FRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);

enum Slot {
    Loading,
    Ready(Arc<RenderImage>),
    Failed(Instant),
}

fn slots() -> std::sync::MutexGuard<'static, HashMap<String, Slot>> {
    static SLOTS: std::sync::LazyLock<Mutex<HashMap<String, Slot>>> = std::sync::LazyLock::new(Default::default);
    SLOTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The address the picture is fetched from, or `None` when `image` is not
/// one this app fetches. Only Google's picture host over https, and nothing
/// in the address that could be read as anything but an address. A trailing
/// size (`=s96-c`) is replaced with the size drawn here; an address without
/// one is left exactly as it is.
pub fn address(image: &str) -> Option<String> {
    let rest = image.strip_prefix("https://")?;
    if image.len() > 2048
        || !image
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '<' | '>' | '{' | '}' | '`'))
    {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    let named = host
        .strip_suffix(".googleusercontent.com")
        .is_some_and(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'));
    if !named || host.contains("..") || host.starts_with(['.', '-']) {
        return None;
    }
    let sized = image.rfind("=s").and_then(|at| {
        let digits = image[at + 2..].strip_suffix("-c")?;
        (!digits.is_empty() && digits.len() <= 4 && digits.chars().all(|c| c.is_ascii_digit()))
            .then(|| format!("{}=s{EDGE}-c", &image[..at]))
    });
    Some(sized.unwrap_or_else(|| image.to_owned()))
}

/// The picture's bytes as an image, or `None` when they are not one of the
/// formats a photograph comes in. Never an SVG: that is a document.
fn decode(bytes: Vec<u8>, cx: &App) -> Option<Arc<RenderImage>> {
    let format = match bytes.as_slice() {
        [0xFF, 0xD8, 0xFF, ..] => ImageFormat::Jpeg,
        [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => ImageFormat::Webp,
        _ => return None,
    };
    if bytes.len() > LIMIT {
        return None;
    }
    let image = Image::from_bytes(format, bytes).to_image_data(cx.svg_renderer()).ok()?;
    let size = image.size(0);
    (size.width.0 > 0 && size.height.0 > 0 && size.width.0 <= 2048 && size.height.0 <= 2048).then_some(image)
}

/// Where a picture is kept between launches. `None` without a data
/// directory, and in tests, which never touch a real one.
fn saved_path(address: &str) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let root = crate::accounts::Paths::of_environment()?.portraits();
    Some(root.join(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, address.as_bytes()).to_string()))
}

fn save(address: &str, bytes: &[u8]) {
    let Some(path) = saved_path(address) else { return };
    let Some(directory) = path.parent() else { return };
    let partial = path.with_extension("part");
    if std::fs::create_dir_all(directory).is_ok() && std::fs::write(&partial, bytes).is_ok() {
        let _ = std::fs::rename(&partial, &path);
    }
}

/// The saved picture and whether it is recent enough to go on using as is.
fn saved(address: &str) -> Option<(Vec<u8>, bool)> {
    let path = saved_path(address)?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 || meta.len() > LIMIT as u64 {
        return None;
    }
    let fresh = meta
        .modified()
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < FRESH);
    Some((std::fs::read(path).ok()?, fresh))
}

/// The picture of the person this session belongs to, when there is one to
/// draw right now. `None` means draw their initial: it is not loaded yet
/// (the window is drawn again when it is), or there is none.
pub fn of(session: &AuthSessionDTO, cx: &mut App) -> Option<Arc<RenderImage>> {
    let address = address(session.user.image.as_deref()?)?;
    let mut known = slots();
    match known.get(&address) {
        Some(Slot::Ready(picture)) => return Some(picture.clone()),
        Some(Slot::Loading) => return None,
        Some(Slot::Failed(at)) if at.elapsed() < RETRY => return None,
        _ => {}
    }
    let stored = saved(&address).and_then(|(bytes, fresh)| Some((decode(bytes, cx)?, fresh)));
    if let Some((picture, fresh)) = &stored {
        known.insert(address.clone(), Slot::Ready(picture.clone()));
        if *fresh {
            return Some(picture.clone());
        }
    } else {
        known.insert(address.clone(), Slot::Loading);
    }
    drop(known);
    let kept = stored.map(|(picture, _)| picture);
    if cfg!(test) {
        return kept;
    }
    let asked = address.clone();
    let reply = cx
        .background_executor()
        .spawn(async move { crate::fetch::bytes(&asked, LIMIT) });
    let showing = kept.clone();
    cx.spawn(async move |cx| {
        let bytes = reply.await.ok().filter(|reply| reply.status == 200).map(|reply| reply.body);
        let _ = cx.update(|cx| {
            let picture = bytes.and_then(|bytes| {
                let picture = decode(bytes.clone(), cx)?;
                save(&address, &bytes);
                Some(picture)
            });
            // A saved picture that could not be renewed is still the picture.
            let slot = match picture.or(showing) {
                Some(picture) => Slot::Ready(picture),
                None => Slot::Failed(Instant::now()),
            };
            slots().insert(address, slot);
            cx.refresh_windows();
        });
    })
    .detach();
    kept
}

/// A person's round avatar, `size` points across: their picture when `of`
/// has one, else `initial` on the tinted fill, as it has always been drawn.
pub fn avatar(picture: Option<Arc<RenderImage>>, size: f32, initial: Div) -> Div {
    match picture {
        Some(picture) => div().size(px(size)).flex_shrink_0().child(
            img(picture)
                .size(px(size))
                .rounded_full()
                .object_fit(ObjectFit::Cover),
        ),
        None => initial,
    }
}

/// For layout review only (`CEREAL_TOUR_PORTRAIT=<image file>` in a debug
/// build): someone signed in whose picture is that file, so the stills can
/// show an avatar without a server or the network.
pub fn tour_session(cx: &mut App) -> Option<AuthSessionDTO> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let file = crate::environment::var_os("CEREAL_TOUR_PORTRAIT")?;
    let image = "https://lh3.googleusercontent.com/a/tour-fixture=s96-c";
    let key = address(image)?;
    if !slots().contains_key(&key) {
        let slot = match std::fs::read(file).ok().and_then(|bytes| decode(bytes, cx)) {
            Some(picture) => Slot::Ready(picture),
            None => Slot::Failed(Instant::now()),
        };
        slots().insert(key, slot);
    }
    serde_json::from_value(serde_json::json!({
        "expiresAt": null,
        "user": {"createdAt": "2026-11-01T00:00:00Z", "email": "sam@acme.example", "id": "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01", "name": "Sam Carter", "role": "owner", "image": image}
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::address;

    #[test]
    fn only_googles_picture_host_over_https_is_ever_asked() {
        for image in [
            "https://lh3.googleusercontent.com/a/ACg8ocFixture",
            "https://lh3.googleusercontent.com/a-/ALV-UjFixture=s96-c",
            "https://lh5.ggpht.googleusercontent.com/photo.jpg",
        ] {
            assert!(address(image).is_some(), "{image}");
        }
        for image in [
            "",
            "lh3.googleusercontent.com/a/x",
            "http://lh3.googleusercontent.com/a/x",
            "file:///etc/passwd",
            "https://googleusercontent.com/a/x",
            "https://.googleusercontent.com/a/x",
            "https://evil.example/a/x",
            "https://lh3.googleusercontent.com.evil.example/a/x",
            "https://evil.example/lh3.googleusercontent.com/a/x",
            "https://evil.example#.googleusercontent.com/a/x",
            "https://evil.example?.googleusercontent.com",
            "https://user@lh3.googleusercontent.com/a/x",
            "https://user:pass@lh3.googleusercontent.com/a/x",
            "https://evil.example\\@lh3.googleusercontent.com/a/x",
            "https://lh3.googleusercontent.com:8443/a/x",
            "https://lh3.googleusercontent.com/a/x\noutput = \"/tmp/x\"",
            "https://lh3.googleusercontent.com/a/x\" --output \"/tmp/x",
            "https://lh3.googleusercontent.com/a/{a,b}",
            "https://lh3.googleusercontent.com/a/x y",
            "https://lh3.googleusercontent.com/a/é",
        ] {
            assert_eq!(address(image), None, "{image:?}");
        }
        assert_eq!(address(&format!("https://lh3.googleusercontent.com/{}", "a".repeat(2048))), None);
    }

    #[test]
    fn a_size_is_replaced_only_where_the_address_already_has_one() {
        assert_eq!(
            address("https://lh3.googleusercontent.com/a/ACg8ocFixture=s96-c").as_deref(),
            Some("https://lh3.googleusercontent.com/a/ACg8ocFixture=s128-c")
        );
        assert_eq!(
            address("https://lh3.googleusercontent.com/a/b=s=s1024-c").as_deref(),
            Some("https://lh3.googleusercontent.com/a/b=s=s128-c")
        );
        for untouched in [
            "https://lh3.googleusercontent.com/a/ACg8ocFixture",
            "https://lh3.googleusercontent.com/a/ACg8ocFixture=s96",
            "https://lh3.googleusercontent.com/a/ACg8ocFixture=w96-h96",
            "https://lh3.googleusercontent.com/a/ACg8ocFixture=s-c",
            "https://lh3.googleusercontent.com/a/photo=sunset-c",
            "https://lh3.googleusercontent.com/a/x=s96-c/photo.jpg",
        ] {
            assert_eq!(address(untouched).as_deref(), Some(untouched));
        }
    }

    #[test]
    fn the_picture_request_carries_nothing_about_who_asks() {
        let source = include_str!("fetch.rs");
        let start = source.find("pub fn bytes(").unwrap();
        let body = &source[start..start + source[start..].find("\n}\n").unwrap()];
        for needle in ["header", "cookie", "referer", "user", "Authorization"] {
            assert!(!body.contains(needle), "the picture request sets {needle}");
        }
        assert!(crate::fetch::bytes("https://lh3.googleusercontent.com/a\nheader = \"X: y\"", 16).is_err());
    }
}
