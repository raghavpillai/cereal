//! Settings › Workspace: the workspace's name (General) and the people in it
//! (Members), for inviting and managing teammates from the Mac.
//!
//! Everything here is the default account's server's: the workspace is read
//! from `/api/v1/organizations` (the active one, else the first) and read
//! again after every change, so the page never shows what the server has not
//! said. What each person may do to each row is decided by the server
//! (`apps/api/src/organizations.ts`); the rules are repeated here only so
//! that an action that would be refused is not offered.
use crate::{
    accounts::{AccountClient, AccountId, client::path_id},
    app::{Cereal, Menu, Screen},
    input::{InputSubmitted, TextInput},
    screens::SettingsTab,
    ui::*,
};
use gpui::{prelude::*, *};
use meeting_notes_api_types::{
    InviteOrganizationMemberInput, InviteOrganizationMemberInputRole as InviteRole, OrganizationDetailsDTO,
    OrganizationDtoRole as Role, OrganizationInvitationDTO, OrganizationInvitationDtoStatus as InvitationStatus,
    OrganizationMemberDTO, OrganizationsDTO, UpdateOrganizationInput, UpdateOrganizationMemberRoleInput,
};

/// The Members page's geometry in points, measured from the reference
/// capture (1858 px across a 1470 pt window: 1.264 px to the point).
pub mod size {
    /// The page's left edge to its text, and the toolbar's right inset.
    pub const LEFT: f32 = 32.;
    pub const TOOLBAR_RIGHT: f32 = 36.;
    /// A row's right inset: the "…" button's 24 pt end 32 pt from the edge.
    pub const ROW_RIGHT: f32 = 32.;
    /// The title's box to the toolbar. The reference has a row of tabs (All,
    /// Groups) between the two, which this page does not: the toolbar takes
    /// the tabs' place, 24.5 pt under the title's baseline.
    pub const TITLE_TO_TOOLBAR: f32 = 18.;
    pub const SEARCH_WIDTH: f32 = 222.;
    pub const CONTROL: f32 = 28.;
    pub const FIELD_RADIUS: f32 = 5.;
    pub const TOOLBAR_TO_HEADER: f32 = 16.;
    pub const HEADER: f32 = 32.;
    pub const BAND: f32 = 32.;
    pub const ROW: f32 = 44.;
    pub const FACE: f32 = 28.;
    pub const FACE_TO_NAME: f32 = 12.;
    /// The email and status columns, as shares of the row between its insets
    /// (367 pt and 180 pt of 1210 pt in the reference).
    pub const EMAIL_SHARE: f32 = 0.303;
    pub const STATUS_SHARE: f32 = 0.149;
    pub const STATUS_MIN: f32 = 96.;
    pub const CHIP: f32 = 16.;
    pub const CHIP_PAD: f32 = 6.;
    pub const CHIP_RADIUS: f32 = 4.;
    pub const MORE: f32 = 24.;
    /// The row menu: 198 pt of surface inside the hairline.
    pub const MENU: f32 = 200.;
    /// General: the name field at the end of its row.
    pub const NAME_FIELD: f32 = 192.;
    pub const NAME_ROW: f32 = 58.;
    pub const DIALOG: f32 = 440.;
}

/// The longest name the server takes (`OrganizationName` in the contracts).
const NAME_LIMIT: usize = 100;
/// The longest address the server takes (`InviteOrganizationMemberInput`).
const ADDRESS_LIMIT: usize = 254;

/// Who a row's menu is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Member(String),
    Invitation(String),
}

/// What a row's "…" menu can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    MakeAdmin,
    MakeMember,
    Remove,
    CancelInvitation,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::MakeAdmin => "Make admin",
            Action::MakeMember => "Make member",
            Action::Remove => "Remove from workspace…",
            Action::CancelInvitation => "Cancel invitation",
        }
    }

    pub fn destructive(self) -> bool {
        matches!(self, Action::Remove | Action::CancelInvitation)
    }
}

/// Owners and admins rename the workspace, invite people and see who has
/// been invited (`update`, `invite` and `detail` on the server).
pub fn may_manage(viewer: &Role) -> bool {
    matches!(viewer, Role::Owner | Role::Admin)
}

/// The roles `viewer` may invite someone as: only the owner invites admins.
pub fn invite_roles(viewer: &Role) -> &'static [InviteRole] {
    match viewer {
        Role::Owner => &[InviteRole::Member, InviteRole::Admin],
        Role::Admin => &[InviteRole::Member],
        Role::Member => &[],
    }
}

/// What `viewer` may do to a member whose role is `target`, in a workspace
/// with `owners` owners. As on the server: only the owner changes roles, and
/// never an owner's; the owner removes anyone but the last owner; an admin
/// removes members only.
pub fn member_actions(viewer: &Role, target: &Role, owners: usize) -> Vec<Action> {
    let mut actions = Vec::new();
    if *viewer == Role::Owner {
        match target {
            Role::Member => actions.push(Action::MakeAdmin),
            Role::Admin => actions.push(Action::MakeMember),
            Role::Owner => {}
        }
    }
    let may_remove = match viewer {
        Role::Owner => true,
        Role::Admin => *target == Role::Member,
        Role::Member => false,
    };
    let last_owner = *target == Role::Owner && owners <= 1;
    if may_remove && !last_owner {
        actions.push(Action::Remove);
    }
    actions
}

/// What `viewer` may do to a pending invitation to join as `invited_as`:
/// the owner cancels any, an admin only those for members.
pub fn invitation_actions(viewer: &Role, invited_as: &Role) -> Vec<Action> {
    let may_cancel = match viewer {
        Role::Owner => true,
        Role::Admin => *invited_as == Role::Member,
        Role::Member => false,
    };
    if may_cancel { vec![Action::CancelInvitation] } else { Vec::new() }
}

pub fn role_label(role: &Role) -> &'static str {
    match role {
        Role::Owner => "Owner",
        Role::Admin => "Admin",
        Role::Member => "Member",
    }
}

/// Whether `text` is an address the server takes: the contract's
/// `^[^\s@]+@[^\s@]+\.[^\s@]+$`, at most 254 characters.
pub fn is_address(text: &str) -> bool {
    if text.chars().count() > ADDRESS_LIMIT || text.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = text.split_once('@') else { return false };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    // A dot with something on both sides of it.
    domain
        .char_indices()
        .any(|(at, c)| c == '.' && at > 0 && at + 1 < domain.len())
}

/// What was typed into the invite field, taken apart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addresses {
    /// Addresses to invite, lower-cased as the server keeps them, each once,
    /// in the order typed.
    pub valid: Vec<String>,
    /// Pieces that are not addresses, as typed.
    pub invalid: Vec<String>,
}

/// The addresses in an invite field: separated by commas, semicolons, spaces
/// or line breaks.
pub fn parse_addresses(text: &str) -> Addresses {
    let mut addresses = Addresses::default();
    for piece in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';')) {
        if piece.is_empty() {
            continue;
        }
        if is_address(piece) {
            let address = piece.to_lowercase();
            if !addresses.valid.contains(&address) {
                addresses.valid.push(address);
            }
        } else if !addresses.invalid.iter().any(|known| known == piece) {
            addresses.invalid.push(piece.to_owned());
        }
    }
    addresses
}

/// Whether someone is found by `query`: part of their name or address, in
/// any case. An empty query finds everyone.
pub fn matches(query: &str, name: &str, email: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || name.to_lowercase().contains(&query) || email.to_lowercase().contains(&query)
}

fn rank(role: &Role) -> u8 {
    match role {
        Role::Owner => 0,
        Role::Admin => 1,
        Role::Member => 2,
    }
}

/// The two groups of the table.
#[derive(Debug, Default, PartialEq)]
pub struct Listing<'a> {
    /// Invitations nobody has answered, oldest first.
    pub invited: Vec<&'a OrganizationInvitationDTO>,
    /// The owner, then admins, then members; each in the order they joined.
    pub members: Vec<&'a OrganizationMemberDTO>,
}

/// The rows the table shows for `query`: pending invitations and members.
pub fn listing<'a>(details: &'a OrganizationDetailsDTO, query: &str) -> Listing<'a> {
    let mut invited: Vec<&OrganizationInvitationDTO> = details
        .invitations
        .iter()
        .filter(|invitation| invitation.status == InvitationStatus::Pending)
        .filter(|invitation| matches(query, "", &invitation.email))
        .collect();
    invited.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.email.to_lowercase().cmp(&b.email.to_lowercase()))
    });
    let mut members: Vec<&OrganizationMemberDTO> = details
        .members
        .iter()
        .filter(|member| matches(query, &member.name, &member.email))
        .collect();
    members.sort_by(|a, b| {
        rank(&a.role)
            .cmp(&rank(&b.role))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Listing { invited, members }
}

/// The workspace an account is in: its active one, else the first listed.
pub fn chosen(list: &OrganizationsDTO) -> Option<&str> {
    list.active_organization_id
        .as_deref()
        .filter(|active| list.organizations.iter().any(|organization| organization.id == *active))
        .or_else(|| list.organizations.first().map(|organization| organization.id.as_str()))
}

fn id_in_path(id: &str) -> Result<String, String> {
    path_id(id).map_err(|error| error.message())
}

/// The account's workspace, its members and (for owners and admins) its
/// invitations.
pub(crate) fn read(client: &AccountClient) -> Result<OrganizationDetailsDTO, String> {
    let list: OrganizationsDTO = client.request("GET", "/api/v1/organizations", None)?;
    let id = chosen(&list).ok_or("You are not in a workspace on this server yet.")?;
    client.request("GET", &format!("/api/v1/organizations/{}", id_in_path(id)?), None)
}

pub(crate) fn rename(client: &AccountClient, organization: &str, name: &str) -> Result<(), String> {
    let body = serde_json::to_value(UpdateOrganizationInput { name: name.to_owned() }).ok();
    client
        .request::<serde_json::Value>("PATCH", &format!("/api/v1/organizations/{organization}"), body)
        .map(|_| ())
}

/// Invite each address in turn, and say how each went.
pub(crate) fn invite(
    client: &AccountClient,
    organization: &str,
    addresses: &[String],
    role: &InviteRole,
) -> Vec<(String, Result<(), String>)> {
    addresses
        .iter()
        .map(|email| {
            let body = serde_json::to_value(InviteOrganizationMemberInput { email: email.clone(), role: role.clone() }).ok();
            let sent = client
                .request::<serde_json::Value>("POST", &format!("/api/v1/organizations/{organization}/invitations"), body)
                .map(|_| ());
            (email.clone(), sent)
        })
        .collect()
}

pub(crate) fn cancel_invitation(client: &AccountClient, organization: &str, invitation: &str) -> Result<(), String> {
    let invitation = id_in_path(invitation)?;
    client
        .request::<serde_json::Value>(
            "DELETE",
            &format!("/api/v1/organizations/{organization}/invitations/{invitation}"),
            None,
        )
        .map(|_| ())
}

pub(crate) fn set_role(client: &AccountClient, organization: &str, member: &str, role: InviteRole) -> Result<(), String> {
    let member = id_in_path(member)?;
    let body = serde_json::to_value(UpdateOrganizationMemberRoleInput { role }).ok();
    client
        .request::<serde_json::Value>("PATCH", &format!("/api/v1/organizations/{organization}/members/{member}"), body)
        .map(|_| ())
}

pub(crate) fn remove_member(client: &AccountClient, organization: &str, member: &str) -> Result<(), String> {
    let member = id_in_path(member)?;
    client
        .request::<serde_json::Value>("DELETE", &format!("/api/v1/organizations/{organization}/members/{member}"), None)
        .map(|_| ())
}

/// What the invite dialog says after a send: nothing when every invitation
/// went, else one line per address that did not, in the server's words.
pub fn invite_problems(results: &[(String, Result<(), String>)]) -> Vec<String> {
    let alone = results.len() == 1;
    results
        .iter()
        .filter_map(|(email, sent)| {
            let error = sent.as_ref().err()?;
            // The server's own refusals name the address; a failure to reach it does not.
            Some(if alone || error.contains(email.as_str()) { error.clone() } else { format!("{email}: {error}") })
        })
        .collect()
}

pub struct Organizations {
    pub details: Option<OrganizationDetailsDTO>,
    /// The account the workspace was read with.
    pub account: Option<AccountId>,
    pub loading: bool,
    pub problem: Option<String>,
    /// A change is on its way to the server: nothing else is sent meanwhile.
    pub busy: bool,
    pub search: Entity<TextInput>,
    pub name: Entity<TextInput>,
    pub name_problem: Option<String>,
    pub invite: Entity<TextInput>,
    pub invite_role: InviteRole,
    pub invite_problems: Vec<String>,
    /// Whose "…" menu is open, and where the pointer opened it.
    pub row_menu: Option<(Target, Point<Pixels>)>,
    /// The member the confirmation is about, by membership id.
    pub removing: Option<String>,
    /// Made-up people for the layout stills: no server is asked anything.
    pub fixture: bool,
    /// Whether a Workspace page was on screen in the last frame.
    on_page: bool,
    /// Counts the reads made, so a late answer to an earlier one is dropped.
    epoch: u64,
    /// Counts the changes sent, for the same reason.
    change: u64,
}

impl Organizations {
    pub fn new(cx: &mut Context<Cereal>) -> (Self, Vec<Subscription>) {
        let search = cx.new(|cx| TextInput::new("Search members", false, cx));
        let name = cx.new(|cx| TextInput::new("Workspace name", false, cx));
        let invite = cx.new(|cx| TextInput::new("name@example.com, name@example.com", false, cx));
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.observe(&invite, |_, _, cx| cx.notify()),
            cx.subscribe(&name, |this, _, _: &InputSubmitted, cx| this.rename_organization(cx)),
            cx.subscribe(&invite, |this, _, _: &InputSubmitted, cx| this.send_invitations(cx)),
        ];
        (
            Self {
                details: None,
                account: None,
                loading: false,
                problem: None,
                busy: false,
                search,
                name,
                name_problem: None,
                invite,
                invite_role: InviteRole::Member,
                invite_problems: Vec::new(),
                row_menu: None,
                removing: None,
                fixture: false,
                on_page: false,
                epoch: 0,
                change: 0,
            },
            subscriptions,
        )
    }

    /// Let go of a workspace that is not the signed-in account's (any more).
    /// Answers still on their way are dropped when they arrive.
    fn forget(&mut self) {
        self.details = None;
        self.account = None;
        self.loading = false;
        self.problem = None;
        self.busy = false;
        self.name_problem = None;
        self.invite_problems.clear();
        self.row_menu = None;
        self.removing = None;
        self.epoch = self.epoch.wrapping_add(1);
        self.change = self.change.wrapping_add(1);
    }

    fn viewer(&self) -> Option<&Role> {
        self.details.as_ref().map(|details| &details.organization.role)
    }
}

fn is_organization_menu(menu: &Option<Menu>) -> bool {
    matches!(menu, Some(Menu::Member | Menu::Invite | Menu::RemoveMember))
}

impl Cereal {
    /// Whether Settings lists the Workspace pages: someone is signed in to a
    /// server, which is where a workspace lives.
    pub fn organization_offered(&self) -> bool {
        self.org.fixture || (crate::server::enabled() && crate::accounts::default_session().is_some())
    }

    /// Once a frame: read the workspace when one of its pages comes on
    /// screen, and let go of what belongs to a page that has gone or to an
    /// account that is no longer the one signed in.
    pub fn organization_frame(&mut self, cx: &mut Context<Self>) {
        let on_page =
            self.screen == Screen::Settings && self.current_note.is_none() && self.settings_tab.is_workspace();
        if !on_page {
            self.org.on_page = false;
            if is_organization_menu(&self.menu) {
                self.menu = None;
            }
            return;
        }
        if !self.organization_offered() {
            // Signed out while the page was open: there is no workspace to show.
            self.org.forget();
            self.org.on_page = false;
            self.settings_tab = SettingsTab::Preferences;
            if is_organization_menu(&self.menu) {
                self.menu = None;
            }
            return;
        }
        if self.org.fixture {
            return;
        }
        let account = crate::accounts::default_account()
            .filter(|account| account.shown())
            .map(|account| account.id);
        if self.org.account.is_some() && self.org.account != account {
            self.org.forget();
            if is_organization_menu(&self.menu) {
                self.menu = None;
            }
        }
        let entered = !std::mem::replace(&mut self.org.on_page, true);
        let unread = self.org.details.is_none() && self.org.problem.is_none();
        if (entered || unread) && !self.org.loading {
            self.load_organization(cx);
        }
    }

    /// Read the workspace again from the default account's server.
    pub fn load_organization(&mut self, cx: &mut Context<Self>) {
        if self.org.fixture || !crate::server::enabled() {
            return;
        }
        let Some(account) =
            crate::accounts::default_account().filter(|account| account.shown() && account.is_ready())
        else {
            return;
        };
        if self.org.account.as_ref().is_some_and(|known| *known != account.id) {
            self.org.forget();
        }
        self.org.loading = true;
        self.org.problem = None;
        self.org.epoch = self.org.epoch.wrapping_add(1);
        let (epoch, seen, id) = (self.org.epoch, account.epoch(), account.id.clone());
        cx.spawn(async move |this, cx| {
            let with = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read(&crate::accounts::client(&with)?) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.org.epoch != epoch {
                    return;
                }
                this.org.loading = false;
                // Signed out, or someone else signed in, while it was asked.
                if crate::accounts::epoch_of(&id) == Some(seen) {
                    match result {
                        Ok(details) => this.accept_organization(id, details, cx),
                        Err(error) => this.org.problem = Some(error),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn accept_organization(&mut self, account: AccountId, details: OrganizationDetailsDTO, cx: &mut Context<Self>) {
        // The name field follows the server unless a new name is being typed.
        let typed = self.org.name.read(cx).content();
        let known = self.org.details.as_ref().map(|known| known.organization.name.clone());
        if typed.trim().is_empty() || known.is_none_or(|known| known.trim() == typed.trim()) {
            let name = details.organization.name.clone();
            self.org.name.update(cx, |input, cx| input.set_content(name, cx));
        }
        // A menu or a question about someone who is no longer listed closes.
        let listed = |target: &Target| match target {
            Target::Member(id) => details.members.iter().any(|member| member.id == *id),
            Target::Invitation(id) => details.invitations.iter().any(|invitation| invitation.id == *id),
        };
        if self.menu == Some(Menu::Member) && !self.org.row_menu.as_ref().is_some_and(|(target, _)| listed(target)) {
            self.menu = None;
        }
        if self.menu == Some(Menu::RemoveMember)
            && !self.org.removing.as_ref().is_some_and(|id| listed(&Target::Member(id.clone())))
        {
            self.menu = None;
        }
        self.org.account = Some(account);
        self.org.details = Some(details);
    }

    /// Send one change to the workspace's server off the UI thread, then read
    /// the workspace again. `done` hears how it went unless the account was
    /// signed out or replaced meanwhile. Returns whether it was sent.
    fn change_organization<T: Send + 'static>(
        &mut self,
        work: impl FnOnce(&AccountClient, &str) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.org.fixture || self.org.busy {
            return false;
        }
        let (Some(details), Some(account)) = (&self.org.details, self.org.account.clone()) else {
            return false;
        };
        let Ok(organization) = path_id(&details.organization.id) else { return false };
        let Some(seen) = crate::accounts::epoch_of(&account) else { return false };
        self.org.busy = true;
        self.org.change = self.org.change.wrapping_add(1);
        let change = self.org.change;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let with = account.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::accounts::client(&with).map(|client| work(&client, &organization)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.org.change != change {
                    return;
                }
                this.org.busy = false;
                if this.org.account.as_ref() == Some(&account) && crate::accounts::epoch_of(&account) == Some(seen) {
                    done(this, result, cx);
                    this.load_organization(cx);
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// Save the name in the General page's field.
    pub fn rename_organization(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.org.details else { return };
        if !may_manage(&details.organization.role) {
            return;
        }
        let name = self.org.name.read(cx).content().trim().to_owned();
        if name == details.organization.name.trim() {
            self.org.name_problem = None;
            cx.notify();
            return;
        }
        if name.is_empty() {
            self.org.name_problem = Some("Workspace name is required".into());
            cx.notify();
            return;
        }
        if name.chars().count() > NAME_LIMIT {
            self.org.name_problem = Some(format!("A workspace name is at most {NAME_LIMIT} characters"));
            cx.notify();
            return;
        }
        self.org.name_problem = None;
        self.change_organization(
            move |client, organization| rename(client, organization, &name),
            |this, result, cx| match result.and_then(|renamed| renamed) {
                Ok(()) => {
                    this.show_toast("Workspace renamed", cx);
                    // The sidebar and the team space say the name too.
                    this.load_team_space(cx);
                }
                Err(error) => this.org.name_problem = Some(error),
            },
            cx,
        );
    }

    /// Open the invite dialog with the keyboard in its field. What was typed
    /// and not sent is still there.
    pub fn open_invite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = self.org.viewer() else { return };
        let roles = invite_roles(viewer);
        if roles.is_empty() {
            return;
        }
        if !roles.contains(&self.org.invite_role) {
            self.org.invite_role = InviteRole::Member;
        }
        self.org.invite_problems.clear();
        self.menu = Some(Menu::Invite);
        self.org.invite.read(cx).focus_handle.focus(window);
        cx.notify();
    }

    fn close_invite(&mut self, discard: bool, window: &mut Window, cx: &mut Context<Self>) {
        if discard {
            self.org.invite.update(cx, |input, cx| input.set_content("", cx));
        }
        self.org.invite_problems.clear();
        self.menu = None;
        self.focus.focus(window);
        cx.notify();
    }

    /// Invite everyone named in the dialog's field.
    pub fn send_invitations(&mut self, cx: &mut Context<Self>) {
        if self.menu != Some(Menu::Invite) || self.org.busy {
            return;
        }
        let Some(viewer) = self.org.viewer() else { return };
        let roles = invite_roles(viewer);
        let role = if roles.contains(&self.org.invite_role) { self.org.invite_role.clone() } else { InviteRole::Member };
        if roles.is_empty() {
            return;
        }
        let addresses = parse_addresses(&self.org.invite.read(cx).content());
        if !addresses.invalid.is_empty() {
            self.org.invite_problems = vec![format!("Not an email address: {}", addresses.invalid.join(", "))];
            cx.notify();
            return;
        }
        if addresses.valid.is_empty() {
            self.org.invite_problems = vec!["Enter an email address to invite.".into()];
            cx.notify();
            return;
        }
        self.org.invite_problems.clear();
        let sending = addresses.valid;
        self.change_organization(
            move |client, organization| invite(client, organization, &sending, &role),
            |this, result, cx| {
                let results = match result {
                    Ok(results) => results,
                    Err(error) => {
                        this.org.invite_problems = vec![error];
                        return;
                    }
                };
                let sent = results.iter().filter(|(_, sent)| sent.is_ok()).count();
                if sent > 0 {
                    this.show_toast(if sent == 1 { "Invitation added" } else { "Invitations added" }, cx);
                }
                let problems = invite_problems(&results);
                // Those that did not go stay in the field, to correct or try again.
                let left: Vec<&str> = results
                    .iter()
                    .filter(|(_, sent)| sent.is_err())
                    .map(|(email, _)| email.as_str())
                    .collect();
                let left = left.join(", ");
                this.org.invite.update(cx, |input, cx| input.set_content(left, cx));
                if problems.is_empty() && this.menu == Some(Menu::Invite) {
                    this.menu = None;
                }
                this.org.invite_problems = problems;
            },
            cx,
        );
    }

    /// What the viewer may do to a row, by the server's rules. Nothing while
    /// the row is not (or no longer) listed.
    pub fn organization_actions(&self, target: &Target) -> Vec<Action> {
        let Some(details) = &self.org.details else { return Vec::new() };
        let viewer = &details.organization.role;
        match target {
            Target::Member(id) => {
                let owners = details.members.iter().filter(|member| member.role == Role::Owner).count();
                details
                    .members
                    .iter()
                    .find(|member| member.id == *id)
                    .map(|member| member_actions(viewer, &member.role, owners))
                    .unwrap_or_default()
            }
            Target::Invitation(id) => details
                .invitations
                .iter()
                .find(|invitation| invitation.id == *id && invitation.status == InvitationStatus::Pending)
                .map(|invitation| invitation_actions(viewer, &invitation.role))
                .unwrap_or_default(),
        }
    }

    pub fn open_member_menu(&mut self, target: Target, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.organization_actions(&target).is_empty() {
            return;
        }
        self.org.row_menu = Some((target, at));
        self.menu = Some(Menu::Member);
        cx.notify();
    }

    fn run_organization_action(&mut self, action: Action, target: Target, cx: &mut Context<Self>) {
        self.menu = None;
        // The row may have changed since the menu was drawn.
        if !self.organization_actions(&target).contains(&action) {
            cx.notify();
            return;
        }
        match (action, target) {
            (Action::MakeAdmin, Target::Member(id)) => self.set_member_role(id, InviteRole::Admin, cx),
            (Action::MakeMember, Target::Member(id)) => self.set_member_role(id, InviteRole::Member, cx),
            // Removing someone is asked about first.
            (Action::Remove, Target::Member(id)) => {
                self.org.removing = Some(id);
                self.menu = Some(Menu::RemoveMember);
            }
            (Action::CancelInvitation, Target::Invitation(id)) => {
                self.change_organization(
                    move |client, organization| cancel_invitation(client, organization, &id),
                    |this, result, cx| match result.and_then(|cancelled| cancelled) {
                        Ok(()) => this.show_toast("Invitation cancelled", cx),
                        Err(error) => this.show_toast(&error, cx),
                    },
                    cx,
                );
            }
            _ => {}
        }
        cx.notify();
    }

    fn set_member_role(&mut self, member: String, role: InviteRole, cx: &mut Context<Self>) {
        let said = if role == InviteRole::Admin { "Made an admin" } else { "Made a member" };
        self.change_organization(
            move |client, organization| set_role(client, organization, &member, role),
            move |this, result, cx| match result.and_then(|changed| changed) {
                Ok(()) => this.show_toast(said, cx),
                Err(error) => this.show_toast(&error, cx),
            },
            cx,
        );
    }

    /// The confirmation's Remove button.
    fn remove_confirmed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.focus.focus(window);
        let Some(member) = self.org.removing.take() else { return };
        if !self.organization_actions(&Target::Member(member.clone())).contains(&Action::Remove) {
            cx.notify();
            return;
        }
        self.change_organization(
            move |client, organization| remove_member(client, organization, &member),
            |this, result, cx| match result.and_then(|removed| removed) {
                Ok(()) => this.show_toast("Removed from the workspace", cx),
                Err(error) => this.show_toast(&error, cx),
            },
            cx,
        );
        cx.notify();
    }

    /// Who is looking: the person signed in to the default account.
    fn organization_me(&self, cx: &mut Context<Self>) -> Option<meeting_notes_api_types::AuthSessionDTO> {
        crate::accounts::default_session()
            .filter(|_| crate::server::enabled())
            .or_else(|| crate::portrait::tour_session(cx))
    }

    /// The "Workspace" caption over the two workspace rows of the Settings
    /// sidebar: 12 pt, 16 pt in from the edge, its middle 21 pt under the row
    /// above and 13 pt over the row below, as in the reference.
    pub(crate) fn organization_sidebar_label(&self) -> Div {
        let t = self.theme();
        div()
            // The rows are 8 pt in and 1 pt apart.
            .mt(px(12.))
            .mb(px(4.))
            .pl(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(t.muted)
            .child("Workspace")
    }

    /// A 28 pt text field: the card colour inside a hairline that turns to
    /// the focus colour while it is typed in.
    fn organization_field(&self, input: &Entity<TextInput>, window: &Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let focused = input.read(cx).focus_handle.is_focused(window);
        div()
            .h(px(size::CONTROL))
            .flex_shrink_0()
            .px(px(9.))
            .flex()
            .items_center()
            .rounded(px(size::FIELD_RADIUS))
            .bg(t.card)
            .border_1()
            .border_color(if focused { focus_ring() } else { t.border })
            .text_size(px(13.))
            // A text input is at least 22 pt tall with its line at the top:
            // a 22 pt line keeps what is typed on the field's middle.
            .line_height(px(22.))
            .child(div().flex_1().min_w_0().child(input.clone()))
    }

    /// Settings › Workspace › General: the workspace's name, which its owner
    /// and admins can change.
    pub(crate) fn organization_general(&self, column: Div, window: &Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let card = crate::screens::settings_card(t);
        let label = |title: &'static str, description: Option<&'static str>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(13.)).line_height(px(18.)).text_color(t.text).child(title))
                .when_some(description, |label, description| label.child(small(description, t)))
        };
        let row = || {
            div()
                .min_h(px(size::NAME_ROW))
                .px(px(16.))
                .py(px(12.))
                .flex()
                .items_center()
                .gap(px(14.))
        };
        let card = match &self.org.details {
            Some(details) if may_manage(&details.organization.role) => {
                let typed = self.org.name.read(cx).content();
                let changed = typed.trim() != details.organization.name.trim();
                card.child(
                    row()
                        .child(label("Workspace name", None))
                        .when(changed, |row| {
                            row.child(
                                pill("org-name-save", if self.org.busy { "Saving…" } else { "Save" }, "", t)
                                    .flex_shrink_0()
                                    .when(self.org.busy, |save| save.opacity(DISABLED))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.focus.focus(window);
                                        this.rename_organization(cx);
                                    })),
                            )
                        })
                        .child(self.organization_field(&self.org.name, window, cx).w(px(size::NAME_FIELD))),
                )
            }
            Some(details) => card.child(
                row()
                    .child(label("Workspace name", Some("The workspace's owner and admins can change it.")))
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(size::NAME_FIELD))
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(t.secondary)
                            .child(details.organization.name.clone()),
                    ),
            ),
            None => card.child(match &self.org.problem {
                Some(problem) => crate::screens::settings_row("warning", "Workspace unavailable", problem.clone(), t)
                    .child(self.organization_retry("org-general-retry", cx)),
                None => row()
                    .child(crate::motion::cereal_loader("org-general-loading", 16., t.muted))
                    .child(small("Loading your workspace…", t)),
            }),
        };
        column
            .child(crate::screens::section_label("Workspace setup", t))
            .child(card)
            .when_some(self.org.name_problem.clone(), |column, problem| {
                column.child(
                    div()
                        .mt(px(8.))
                        .px(px(9.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.danger)
                        .child(problem),
                )
            })
    }

    fn organization_retry(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        pill(id, "Try again", "refresh", self.theme())
            .flex_shrink_0()
            .on_click(cx.listener(|this, _, window, cx| {
                this.focus.focus(window);
                this.load_organization(cx);
                cx.notify();
            }))
    }

    /// Settings › Workspace › Members: a search, "Invite teammates", and the
    /// table of who has been invited and who is in.
    pub(crate) fn render_organization_members(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let page = (f32::from(window.viewport_size().width) - self.sidebar_inset()).max(320.);
        let inner = page - size::LEFT - size::ROW_RIGHT;
        let status_width = (inner * size::STATUS_SHARE).round().max(size::STATUS_MIN);
        let email_width = (inner * size::EMAIL_SHARE).round();
        let viewer = self.org.viewer().cloned();
        let query = self.org.search.read(cx).content();
        let scroll = crate::lazy::scroll("workspace-members");

        let invite = viewer.as_ref().is_some_and(|viewer| !invite_roles(viewer).is_empty()).then(|| {
            div()
                .id("org-invite")
                .h(px(size::CONTROL))
                // As measured: the glyph's ink 11 pt in, the label's 31.6 pt
                // in and ending 9.5 pt short of the capsule's end.
                .pl(px(11.))
                .pr(px(7.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(5.))
                .rounded_full()
                .bg(t.text)
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.bg)
                .cursor_default()
                .child(crate::motion::tactile(neutral_wash(0.14)).press(neutral_wash(0.12)).round())
                .child(icon("invite", 15., t.bg))
                .child("Invite teammates")
                .reach()
                .on_click(cx.listener(|this, _, window, cx| this.open_invite(window, cx)))
        });
        let head = div()
            .pt(px(9.))
            .pl(px(size::LEFT))
            .pr(px(size::TOOLBAR_RIGHT))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(heading("Members", t))
                    // Reading again with the table already drawn.
                    .when(self.org.loading && self.org.details.is_some(), |title| {
                        title.child(crate::motion::cereal_loader("org-reloading", 14., t.muted))
                    }),
            )
            .child(
                div()
                    .mt(px(size::TITLE_TO_TOOLBAR))
                    .h(px(size::CONTROL))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    // Its text starts 8.7 pt from the field's edge in the reference.
                    .child(
                        self.organization_field(&self.org.search, window, cx)
                            .px(px(7.))
                            .w(px(size::SEARCH_WIDTH)),
                    )
                    .child(div().flex_1())
                    .children(invite),
            );

        let cell = |text: &'static str| div().text_size(px(12.)).line_height(px(16.)).text_color(t.muted).child(text);
        let header = div()
            .mt(px(size::TOOLBAR_TO_HEADER))
            .h(px(size::HEADER))
            .flex_shrink_0()
            .pl(px(size::LEFT))
            .pr(px(size::ROW_RIGHT))
            .flex()
            .items_center()
            .child(cell("Name").flex_1().min_w_0())
            .child(cell("Email").w(px(email_width)).flex_shrink_0())
            .child(cell("Status").w(px(status_width)).flex_shrink_0());

        let body: AnyElement = match &self.org.details {
            None => {
                let state = div().py(px(56.)).flex().flex_col().items_center().gap(px(12.));
                match &self.org.problem {
                    Some(problem) => state
                        .child(icon("warning", 20., t.muted))
                        .child(
                            div()
                                .max_w(px(420.))
                                .px(px(size::LEFT))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(t.muted)
                                .text_center()
                                .child(problem.clone()),
                        )
                        .child(self.organization_retry("org-members-retry", cx))
                        .into_any_element(),
                    None => state
                        .child(crate::motion::cereal_loader("org-members-loading", 20., t.muted))
                        .into_any_element(),
                }
            }
            Some(details) => {
                enum Entry<'a> {
                    Band(&'static str, usize),
                    Invitation(&'a OrganizationInvitationDTO),
                    Member(&'a OrganizationMemberDTO),
                }
                let listed = listing(details, &query);
                let mut entries = Vec::with_capacity(listed.invited.len() + listed.members.len() + 2);
                if !listed.invited.is_empty() {
                    entries.push(Entry::Band("Invited", listed.invited.len()));
                    entries.extend(listed.invited.iter().map(|invitation| Entry::Invitation(invitation)));
                }
                if !listed.members.is_empty() {
                    entries.push(Entry::Band("Members", listed.members.len()));
                    entries.extend(listed.members.iter().map(|member| Entry::Member(member)));
                }
                let keys: Vec<u64> = entries
                    .iter()
                    .map(|entry| match entry {
                        Entry::Band(label, _) => crate::lazy::key(("band", label)),
                        Entry::Invitation(invitation) => crate::lazy::key(("invitation", &invitation.id)),
                        Entry::Member(member) => crate::lazy::key(("member", &member.id)),
                    })
                    .collect();
                let me = self.organization_me(cx);
                let my_picture = me.as_ref().and_then(|session| crate::portrait::of(session, cx));
                let my_id = me.map(|session| session.user.id);
                let band = band_fill(t);
                let busy = self.org.busy;
                // One person or invitation: a face, a name, an address, what
                // they are, and the menu of what may be done about them.
                let person = |face: Div,
                              name: String,
                              email: String,
                              status: Div,
                              target: Target,
                              cx: &mut Context<Self>| {
                    let more = (!self.organization_actions(&target).is_empty()).then(|| {
                        let key = match &target {
                            Target::Member(id) | Target::Invitation(id) => crate::lazy::key(id),
                        };
                        // The reference's dots are 13.4 pt across: a 20 pt glyph.
                        tool_sized(("org-more", key), "more", 20., t)
                            .size(px(size::MORE))
                            .when(busy, |more| more.opacity(DISABLED))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                if this.org.busy {
                                    return;
                                }
                                let at = window.mouse_position();
                                this.open_member_menu(target.clone(), at, cx);
                            }))
                    });
                    div()
                        .h(px(size::ROW))
                        .flex_shrink_0()
                        .pl(px(size::LEFT))
                        .pr(px(size::ROW_RIGHT))
                        .flex()
                        .items_center()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .pr(px(16.))
                                .flex()
                                .items_center()
                                .gap(px(size::FACE_TO_NAME))
                                .child(face)
                                .child(div().min_w_0().truncate().text_color(t.text).child(name.clone())),
                        )
                        .child(
                            div()
                                .w(px(email_width))
                                .flex_shrink_0()
                                .pr(px(16.))
                                .truncate()
                                .text_color(t.muted)
                                .child(email),
                        )
                        .child(
                            div()
                                .w(px(status_width))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(status)
                                .child(div().flex_1())
                                .child(match more {
                                    Some(more) => more.into_any_element(),
                                    None => div().size(px(size::MORE)).flex_shrink_0().into_any_element(),
                                }),
                        )
                        .into_any_element()
                };
                let rows = crate::lazy::column("workspace-members", &keys, page, 0., &scroll, window, |index| {
                    match &entries[index] {
                        Entry::Band(label, count) => div()
                            .h(px(size::BAND))
                            .flex_shrink_0()
                            .pl(px(size::LEFT))
                            .bg(band)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(t.muted)
                            .child(*label)
                            .child(div().text_color(t.muted.opacity(0.55)).child(count.to_string()))
                            .into_any_element(),
                        Entry::Invitation(invitation) => person(
                            crate::team_space::letter(&invitation.email, &invitation.email, size::FACE, None),
                            invitation.email.clone(),
                            invitation.email.clone(),
                            chip("Invited", false, t),
                            Target::Invitation(invitation.id.clone()),
                            cx,
                        ),
                        Entry::Member(member) => {
                            let key = if member.email.is_empty() { &member.name } else { &member.email };
                            let picture = my_picture
                                .clone()
                                .filter(|_| my_id.as_deref() == Some(member.user_id.as_str()));
                            let name = if member.name.trim().is_empty() { member.email.clone() } else { member.name.clone() };
                            person(
                                crate::portrait::avatar(
                                    picture,
                                    size::FACE,
                                    crate::team_space::letter(&name, key, size::FACE, None),
                                ),
                                name,
                                member.email.clone(),
                                chip(role_label(&member.role), member.role != Role::Member, t),
                                Target::Member(member.id.clone()),
                                cx,
                            )
                        }
                    }
                });
                let nobody = entries.is_empty().then(|| {
                    div()
                        .pl(px(size::LEFT))
                        .pr(px(size::ROW_RIGHT))
                        .py(px(28.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.muted)
                        .child(if query.trim().is_empty() {
                            "Nobody is in this workspace yet.".to_owned()
                        } else {
                            format!("Nobody matches “{}”.", query.trim())
                        })
                });
                div()
                    .flex()
                    .flex_col()
                    // A read that failed with the table still drawn.
                    .when_some(self.org.problem.clone(), |table, problem| {
                        table.child(
                            div()
                                .pl(px(size::LEFT))
                                .pr(px(size::TOOLBAR_RIGHT))
                                .pb(px(10.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(div().flex_1().min_w_0().child(small(problem, t).text_color(t.danger)))
                                .child(self.organization_retry("org-members-retry", cx)),
                        )
                    })
                    .child(rows)
                    .children(nobody)
                    .into_any_element()
            }
        };

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .id("workspace-members-scroll")
                    .track_scroll(&scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .child(crate::motion::page(
                        ("settings-tab", SettingsTab::Members as u64),
                        div()
                            .w_full()
                            .pb(px(48.))
                            .flex()
                            .flex_col()
                            .child(head)
                            .child(header)
                            .child(body),
                    )),
            )
            .child(scroll_thumb("workspace-members", &scroll))
            .into_any_element()
    }

    /// The Members page's menu and dialogs, drawn where every menu is
    /// (`render_menu`): `content` is the popover's surface and `overlay` the
    /// layer a press outside it lands on.
    pub(crate) fn render_organization_menu(
        &self,
        menu: Menu,
        content: Div,
        overlay: Div,
        pose: crate::motion::Pose,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::motion::{Pop, pop};
        let t = self.theme();
        let dialog = |content: Div, overlay: Div| {
            overlay
                .bg(scrim(Scrim::Dialog, pose.fade))
                .flex()
                .items_center()
                .justify_center()
                .child(pop(
                    content.occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
                    Pop::DIALOG,
                    pose,
                ))
                .into_any_element()
        };
        // A dialog's title beside its close button, over a paragraph.
        let titled = |content: Div, title: String, body: String, cx: &mut Context<Self>| {
            content
                .w(px(size::DIALOG))
                .p(px(24.))
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_start()
                        .gap(px(12.))
                        .child(div().flex_1().min_w_0().child(heading(title, t)))
                        .child(tool("close-dialog", "close", t).on_click(cx.listener(|this, _, window, cx| {
                            this.menu = None;
                            this.focus.focus(window);
                            cx.notify();
                        }))),
                )
                .child(div().text_size(px(14.)).line_height(px(21.)).text_color(t.muted).child(body))
        };
        // The dialog's filled button, in the ink or the danger colour.
        let filled = |id: &'static str, label: SharedString, fill: Hsla, enabled: bool| {
            crate::motion::dims(
                div()
                    .id(id)
                    .h(px(28.))
                    .px(px(14.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .bg(fill)
                    .text_color(t.bg)
                    .cursor_default()
                    .when(!enabled, |button| button.opacity(DISABLED))
                    .child(label),
                crate::motion::reveal_key(id),
                if enabled { 0.14 } else { 0. },
            )
            .reach()
        };
        match menu {
            Menu::Member => {
                let Some((target, at)) = self.org.row_menu.clone() else {
                    return overlay.into_any_element();
                };
                let actions = self.organization_actions(&target);
                if actions.is_empty() {
                    return overlay.into_any_element();
                }
                // Rows on a 28 px pitch, labels only, as the note row's menu.
                let mut content = content.w(px(size::MENU)).p(px(4.));
                for action in actions.iter().copied() {
                    let target = target.clone();
                    let run = move |this: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                        this.run_organization_action(action, target.clone(), cx)
                    };
                    content = content.child(
                        if action.destructive() {
                            self.danger_row(action.label(), "", cx, run)
                        } else {
                            self.menu_row(action.label(), "", cx, run)
                        }
                        .px(px(8.)),
                    );
                }
                // Under the "…" that opened it, its trailing edge a little
                // past the pointer.
                let origin = crate::chrome::popover_origin(
                    point(at.x + px(14.) - px(size::MENU), at.y + px(12.)),
                    gpui::size(px(size::MENU), px(actions.len() as f32 * 28. + 10.)),
                    window.viewport_size(),
                );
                overlay
                    .child(div().absolute().left(origin.x).top(origin.y).child(pop(
                        content.occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
                        Pop::menu((0.92, 0.)),
                        pose,
                    )))
                    .into_any_element()
            }
            Menu::RemoveMember => {
                let member = self.org.details.as_ref().and_then(|details| {
                    let id = self.org.removing.as_ref()?;
                    details.members.iter().find(|member| member.id == *id)
                });
                let Some(member) = member else { return overlay.into_any_element() };
                let workspace = self
                    .org
                    .details
                    .as_ref()
                    .map(|details| details.organization.name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "this workspace".into());
                let name = if member.name.trim().is_empty() { member.email.clone() } else { member.name.clone() };
                let content = titled(
                    content,
                    format!("Remove {name}?"),
                    format!(
                        "{} will no longer be a member of {workspace} or see what is shared with it. Their own notes stay theirs, and they can be invited again.",
                        member.email
                    ),
                    cx,
                )
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(pill("org-remove-cancel", "Cancel", "", t).on_click(cx.listener(|this, _, window, cx| {
                            this.menu = None;
                            this.org.removing = None;
                            this.focus.focus(window);
                            cx.notify();
                        })))
                        .child(
                            filled("org-remove-confirm", "Remove".into(), t.danger, true)
                                .on_click(cx.listener(|this, _, window, cx| this.remove_confirmed(window, cx))),
                        ),
                );
                dialog(content, overlay)
            }
            _ => {
                let roles = self.org.viewer().map(invite_roles).unwrap_or_default();
                let busy = self.org.busy;
                let typed = parse_addresses(&self.org.invite.read(cx).content()).valid.len();
                let role = |id: &'static str, label: &'static str, role: InviteRole, cx: &mut Context<Self>| {
                    let selected = self.org.invite_role == role;
                    pill(id, label, "", t)
                        .when(selected, |pill| pill.bg(t.hover))
                        .when(!selected, |pill| pill.text_color(t.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.org.invite_role = role.clone();
                            cx.notify();
                        }))
                };
                let mut choice = div().flex().items_center().gap(px(6.));
                for offered in roles {
                    choice = choice.child(match offered {
                        InviteRole::Member => role("org-invite-member", "Member", InviteRole::Member, cx),
                        InviteRole::Admin => role("org-invite-admin", "Admin", InviteRole::Admin, cx),
                    });
                }
                let content = titled(
                    content,
                    "Invite teammates".into(),
                    invite_explanation(
                        &self.org.account.as_ref().and_then(crate::accounts::get).map(|account| account.host()).unwrap_or_default(),
                    )
                    .into(),
                    cx,
                )
                .child(self.organization_field(&self.org.invite, window, cx).h(px(32.)).w_full())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(div().text_size(px(13.)).line_height(px(18.)).text_color(t.muted).child("Role"))
                        .child(choice),
                )
                .when(!self.org.invite_problems.is_empty(), |content| {
                    let mut problems = div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.danger);
                    for problem in &self.org.invite_problems {
                        problems = problems.child(div().child(problem.clone()));
                    }
                    content.child(problems)
                })
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(pill("org-invite-cancel", "Cancel", "", t).on_click(cx.listener(|this, _, window, cx| {
                            this.close_invite(true, window, cx)
                        })))
                        .child(
                            filled("org-invite-send", send_label(typed, busy).into(), t.text, !busy)
                                .on_click(cx.listener(|this, _, _, cx| this.send_invitations(cx))),
                        ),
                );
                dialog(content, overlay)
            }
        }
    }
}

/// What the invite dialog says under its title. The server sends no email,
/// so it says where the person signs in (`host`, the server as the account
/// knows it).
pub fn invite_explanation(host: &str) -> String {
    let place = if host.is_empty() { "to this server".to_owned() } else { format!("at {host}") };
    format!("No email is sent, so let them know: they sign in {place} to join. This does not share any existing notes.")
}

/// What the invite dialog's button says, for `addresses` typed so far.
/// Nothing is sent to anyone, so it does not say "Send".
pub fn send_label(addresses: usize, sending: bool) -> String {
    match (sending, addresses) {
        (true, _) => "Inviting…".into(),
        (false, 0 | 1) => "Invite".into(),
        (false, several) => format!("Invite {several} people"),
    }
}

/// The fill of a group's band: 5% black on the light page, as measured;
/// the hover wash on the dark one.
fn band_fill(t: Theme) -> Hsla {
    if is_dark() { t.hover } else { rgba(0x0000000d).into() }
}

/// What someone is, as a small tag: quiet for members and invitations,
/// tinted with the accent for the owner and admins.
fn chip(label: &'static str, strong: bool, t: Theme) -> Div {
    div()
        .h(px(size::CHIP))
        .px(px(size::CHIP_PAD))
        .flex_shrink_0()
        .flex()
        .items_center()
        .rounded(px(size::CHIP_RADIUS))
        .bg(if strong { t.accent.opacity(0.15) } else { t.hover })
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(t.muted)
        .child(label)
}

/// Made-up people for the layout stills (`CEREAL_TOUR=p-fix-org`): nobody
/// here exists. The viewer is "Sam Carter", as in `portrait::tour_session`.
#[cfg(debug_assertions)]
pub fn tour_details(viewer: Role) -> OrganizationDetailsDTO {
    let organization = "00000000-0000-4000-8000-000000000002";
    let me = "0b7e1c9e-5a44-4c1d-9d2e-3f6a7b8c9d01";
    let names = [
        "Sam Carter",
        "Oda Dupont",
        "Alexandra Agostini",
        "Omar Sharma",
        "Ilya Faber",
        "Yuval Steiner",
        "Sahar F",
        "Reggie Kung",
        "Bar Mazur",
        "Swan Beaumont",
        "Lokesh Narayan",
        "Sean Jung",
        "Jax Dunphy",
        "Priya Raman",
    ];
    // Whoever is looking, the workspace has one owner and one admin.
    let role_of = |index: usize| match (&viewer, index) {
        (Role::Owner, 0) | (Role::Admin, 1) | (Role::Member, 1) => Role::Owner,
        (Role::Owner, 1) | (Role::Admin, 0) | (Role::Member, 2) => Role::Admin,
        _ => Role::Member,
    };
    let members = names
        .iter()
        .enumerate()
        .map(|(index, name)| OrganizationMemberDTO {
            created_at: format!("2026-10-{:02}T09:00:00.000Z", index + 1),
            email: format!("{}@acme.example", name.split(' ').next().unwrap_or("someone").to_lowercase()),
            id: format!("00000000-0000-4000-8000-0000000001{index:02}"),
            name: (*name).to_owned(),
            organization_id: organization.to_owned(),
            role: role_of(index),
            user_id: if index == 0 { me.to_owned() } else { format!("00000000-0000-4000-8000-0000000002{index:02}") },
        })
        .collect();
    let invitations = if may_manage(&viewer) {
        vec![OrganizationInvitationDTO {
            created_at: "2026-11-17T09:00:00.000Z".into(),
            email: "avi@acme.example".into(),
            expires_at: "2026-11-24T09:00:00.000Z".into(),
            id: "00000000-0000-4000-8000-000000000301".into(),
            inviter_id: me.into(),
            organization_id: organization.into(),
            organization_name: "Acme".into(),
            role: Role::Member,
            status: InvitationStatus::Pending,
        }]
    } else {
        Vec::new()
    };
    OrganizationDetailsDTO {
        invitations,
        members,
        organization: meeting_notes_api_types::OrganizationDTO {
            created_at: "2026-10-01T09:00:00.000Z".into(),
            id: organization.into(),
            name: "Acme".into(),
            role: viewer,
            slug: "acme".into(),
        },
    }
}

impl Cereal {
    /// A state of the Workspace pages to look at, for the scripted tour: no
    /// server is asked anything and nothing is sent. `state` is who is
    /// looking ("owner", "admin", "member"), "loading", "failed", or "off"
    /// to put the pages away again.
    /// Release builds compile the tour too; there it has nothing to show.
    #[cfg(not(debug_assertions))]
    pub fn review_organization(&mut self, _state: &str, _cx: &mut Context<Self>) {}

    #[cfg(debug_assertions)]
    pub fn review_organization(&mut self, state: &str, cx: &mut Context<Self>) {
        self.org.forget();
        self.org.fixture = state != "off";
        self.org.search.update(cx, |input, cx| input.set_content("", cx));
        self.org.invite.update(cx, |input, cx| input.set_content("", cx));
        let viewer = match state {
            "owner" => Some(Role::Owner),
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            _ => None,
        };
        match state {
            "loading" => self.org.loading = true,
            "failed" => {
                self.org.problem = Some(
                    "Server request failed (transport exit 7). Local audio is preserved; retry when connected.".into(),
                )
            }
            _ => {}
        }
        if let Some(viewer) = viewer {
            let details = tour_details(viewer);
            let name = details.organization.name.clone();
            self.org.name.update(cx, |input, cx| input.set_content(name, cx));
            self.org.details = Some(details);
        }
        cx.notify();
    }
}

impl Cereal {
    /// The pages at work on a real workspace, for the scripted tour
    /// (`CEREAL_AUDIT=1 CEREAL_TOUR=a-org`, signed in as the owner of a
    /// throwaway server): `act` opens a page ("members", "general"), saves a
    /// name ("rename:<name>"), sends invitations ("invite:<addresses>"),
    /// closes the dialog ("close") or presses the confirmation's Remove
    /// ("confirm"). These change the workspace, so they only ever run against
    /// a server on this Mac.
    #[cfg(not(debug_assertions))]
    pub fn review_organization_live(&mut self, _act: &str, _window: &mut Window, _cx: &mut Context<Self>) {}

    #[cfg(debug_assertions)]
    pub fn review_organization_live(&mut self, act: &str, window: &mut Window, cx: &mut Context<Self>) {
        let on_this_mac = crate::accounts::default_account().is_some_and(|account| {
            let host = account.host();
            ["127.0.0.1", "localhost", "[::1]"]
                .iter()
                .any(|own| host == *own || host.starts_with(&format!("{own}:")))
        });
        if !on_this_mac {
            eprintln!("tour: the a-org steps change a workspace and run only against a server on this Mac");
            return;
        }
        let (act, value) = act.split_once(':').unwrap_or((act, ""));
        match act {
            "members" => self.open_settings(SettingsTab::Members, cx),
            "general" => self.open_settings(SettingsTab::WorkspaceGeneral, cx),
            "rename" => {
                self.org.name.update(cx, |input, cx| input.set_content(value.to_owned(), cx));
                self.rename_organization(cx);
            }
            "invite" => {
                self.open_invite(window, cx);
                self.org.invite.update(cx, |input, cx| input.set_content(value.to_owned(), cx));
                self.send_invitations(cx);
            }
            "close" => self.close_invite(true, window, cx),
            "confirm" => self.remove_confirmed(window, cx),
            _ => {}
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::client::testing::{Fake, json};
    // `gpui::*` re-exports its own `test` attribute; these are plain unit tests.
    use core::prelude::v1::test;

    const ORGANIZATION: &str = "00000000-0000-4000-8000-000000000002";

    fn id(n: u32) -> String {
        format!("00000000-0000-4000-8000-{n:012}")
    }

    fn member(n: u32, name: &str, role: Role, joined: &str) -> OrganizationMemberDTO {
        OrganizationMemberDTO {
            created_at: joined.into(),
            email: format!("{}@example.test", name.to_lowercase().replace(' ', ".")),
            id: id(n),
            name: name.into(),
            organization_id: ORGANIZATION.into(),
            role,
            user_id: id(1000 + n),
        }
    }

    fn invitation(n: u32, email: &str, role: Role, status: InvitationStatus, created: &str) -> OrganizationInvitationDTO {
        OrganizationInvitationDTO {
            created_at: created.into(),
            email: email.into(),
            expires_at: "2026-12-01T00:00:00.000Z".into(),
            id: id(n),
            inviter_id: id(1001),
            organization_id: ORGANIZATION.into(),
            organization_name: "Acme".into(),
            role,
            status,
        }
    }

    fn details(
        viewer: Role,
        members: Vec<OrganizationMemberDTO>,
        invitations: Vec<OrganizationInvitationDTO>,
    ) -> OrganizationDetailsDTO {
        OrganizationDetailsDTO {
            invitations,
            members,
            organization: meeting_notes_api_types::OrganizationDTO {
                created_at: "2026-10-01T00:00:00.000Z".into(),
                id: ORGANIZATION.into(),
                name: "Acme".into(),
                role: viewer,
                slug: "acme".into(),
            },
        }
    }

    const ROLES: [Role; 3] = [Role::Owner, Role::Admin, Role::Member];

    /// `updateRole` in apps/api/src/organizations.ts, as a predicate.
    fn server_changes_role(actor: &Role, target: &Role) -> bool {
        *actor == Role::Owner && *target != Role::Owner
    }

    /// `remove` in apps/api/src/organizations.ts, as a predicate.
    fn server_removes(actor: &Role, target: &Role, owners: usize) -> bool {
        let forbidden = *actor != Role::Owner && (*actor != Role::Admin || *target != Role::Member);
        let last_owner = *target == Role::Owner && owners <= 1;
        !forbidden && !last_owner
    }

    /// `cancel` in apps/api/src/organizations.ts, as a predicate.
    fn server_cancels(actor: &Role, invited_as: &Role) -> bool {
        !(*actor != Role::Owner && (*actor != Role::Admin || *invited_as != Role::Member))
    }

    #[test]
    fn a_row_offers_exactly_what_the_server_would_allow() {
        for viewer in &ROLES {
            for target in &ROLES {
                for owners in [1, 2] {
                    let actions = member_actions(viewer, target, owners);
                    let changes = actions.iter().any(|action| matches!(action, Action::MakeAdmin | Action::MakeMember));
                    assert_eq!(changes, server_changes_role(viewer, target), "{viewer:?} on {target:?}");
                    assert_eq!(
                        actions.contains(&Action::Remove),
                        server_removes(viewer, target, owners),
                        "{viewer:?} removing {target:?} of {owners}"
                    );
                    // A role change is to the other of the two roles that can be given.
                    assert!(!(actions.contains(&Action::MakeAdmin) && actions.contains(&Action::MakeMember)));
                    assert_eq!(actions.contains(&Action::MakeAdmin), changes && *target == Role::Member);
                    assert_eq!(actions.contains(&Action::MakeMember), changes && *target == Role::Admin);
                    assert!(!actions.contains(&Action::CancelInvitation));
                }
                assert_eq!(
                    invitation_actions(viewer, target) == vec![Action::CancelInvitation],
                    server_cancels(viewer, target),
                    "{viewer:?} cancelling an invitation as {target:?}"
                );
                assert!(invitation_actions(viewer, target).len() <= 1);
            }
        }
        // Spelled out: what each kind of person sees.
        assert_eq!(member_actions(&Role::Owner, &Role::Member, 1), [Action::MakeAdmin, Action::Remove]);
        assert_eq!(member_actions(&Role::Owner, &Role::Admin, 1), [Action::MakeMember, Action::Remove]);
        assert!(member_actions(&Role::Owner, &Role::Owner, 1).is_empty());
        assert_eq!(member_actions(&Role::Owner, &Role::Owner, 2), [Action::Remove]);
        assert_eq!(member_actions(&Role::Admin, &Role::Member, 1), [Action::Remove]);
        assert!(member_actions(&Role::Admin, &Role::Admin, 1).is_empty());
        assert!(member_actions(&Role::Admin, &Role::Owner, 2).is_empty());
        for target in &ROLES {
            assert!(member_actions(&Role::Member, target, 2).is_empty());
            assert!(invitation_actions(&Role::Member, target).is_empty());
        }
        assert!(invitation_actions(&Role::Admin, &Role::Admin).is_empty());
    }

    #[test]
    fn only_owners_and_admins_manage_and_only_the_owner_invites_admins() {
        assert!(may_manage(&Role::Owner) && may_manage(&Role::Admin) && !may_manage(&Role::Member));
        assert_eq!(invite_roles(&Role::Owner), [InviteRole::Member, InviteRole::Admin]);
        assert_eq!(invite_roles(&Role::Admin), [InviteRole::Member]);
        assert!(invite_roles(&Role::Member).is_empty());
        assert!(Action::Remove.destructive() && Action::CancelInvitation.destructive());
        assert!(!Action::MakeAdmin.destructive() && !Action::MakeMember.destructive());
    }

    #[test]
    fn the_invite_field_is_taken_apart_into_addresses() {
        assert_eq!(parse_addresses(""), Addresses::default());
        assert_eq!(parse_addresses("  ,; \n"), Addresses::default());
        let typed = "Ada@Example.test, ben@example.test;cy@example.test\n dee@example.test\tada@example.test";
        assert_eq!(
            parse_addresses(typed).valid,
            ["ada@example.test", "ben@example.test", "cy@example.test", "dee@example.test"]
        );
        assert!(parse_addresses(typed).invalid.is_empty());
        let mixed = parse_addresses("ada@example.test, ben, @example.test, cy@example, ben");
        assert_eq!(mixed.valid, ["ada@example.test"]);
        assert_eq!(mixed.invalid, ["ben", "@example.test", "cy@example"]);
    }

    #[test]
    fn the_send_button_counts_the_addresses_typed() {
        assert_eq!(send_label(0, false), "Invite");
        assert_eq!(send_label(1, false), "Invite");
        assert_eq!(send_label(3, false), "Invite 3 people");
        assert_eq!(send_label(3, true), "Inviting…");
        assert_eq!(
            invite_explanation("notes.example.com"),
            "No email is sent, so let them know: they sign in at notes.example.com to join. This does not share any existing notes."
        );
        assert!(invite_explanation("").contains("they sign in to this server to join"));
    }

    #[test]
    fn an_address_is_what_the_contract_says_it_is() {
        for good in ["a@b.c", "first.last+tag@sub.example.test", "ü@ex.ample"] {
            assert!(is_address(good), "{good}");
        }
        for bad in ["", "a", "a@", "@b.c", "a@b", "a@b.", "a@.c", "a@@b.c", "a@b@c.d", "a b@c.d"] {
            assert!(!is_address(bad), "{bad}");
        }
        let long = format!("{}@example.test", "a".repeat(ADDRESS_LIMIT - "@example.test".len()));
        assert!(is_address(&long));
        assert!(!is_address(&format!("a{long}")));
    }

    #[test]
    fn search_finds_people_by_name_or_address_in_any_case() {
        assert!(matches("", "Ada Lovelace", "ada@example.test"));
        assert!(matches("  ", "Ada Lovelace", "ada@example.test"));
        assert!(matches("LOVE", "Ada Lovelace", "ada@example.test"));
        assert!(matches(" example.TEST ", "Ada Lovelace", "ada@example.test"));
        assert!(!matches("ben", "Ada Lovelace", "ada@example.test"));
        let all = details(
            Role::Owner,
            vec![
                member(1, "Ada Lovelace", Role::Owner, "2026-10-01T00:00:00.000Z"),
                member(2, "Ben Okri", Role::Member, "2026-10-02T00:00:00.000Z"),
            ],
            vec![invitation(9, "benedict@example.test", Role::Member, InvitationStatus::Pending, "2026-11-01T00:00:00.000Z")],
        );
        let found = listing(&all, "ben");
        assert_eq!(found.members.iter().map(|member| member.name.as_str()).collect::<Vec<_>>(), ["Ben Okri"]);
        assert_eq!(found.invited.len(), 1);
        let none = listing(&all, "nobody");
        assert!(none.members.is_empty() && none.invited.is_empty());
    }

    #[test]
    fn the_table_groups_pending_invitations_and_orders_members_by_role_then_joining() {
        let all = details(
            Role::Owner,
            vec![
                member(1, "Zed Member", Role::Member, "2026-10-03T00:00:00.000Z"),
                member(2, "Amy Member", Role::Member, "2026-10-05T00:00:00.000Z"),
                member(3, "Late Admin", Role::Admin, "2026-10-09T00:00:00.000Z"),
                member(4, "The Owner", Role::Owner, "2026-10-10T00:00:00.000Z"),
                member(5, "bea twin", Role::Member, "2026-10-04T00:00:00.000Z"),
                member(6, "Abe Twin", Role::Member, "2026-10-04T00:00:00.000Z"),
            ],
            vec![
                invitation(11, "new@example.test", Role::Member, InvitationStatus::Pending, "2026-11-02T00:00:00.000Z"),
                invitation(12, "old@example.test", Role::Admin, InvitationStatus::Pending, "2026-11-01T00:00:00.000Z"),
                invitation(13, "gone@example.test", Role::Member, InvitationStatus::Canceled, "2026-10-01T00:00:00.000Z"),
                invitation(14, "late@example.test", Role::Member, InvitationStatus::Expired, "2026-10-01T00:00:00.000Z"),
                invitation(15, "in@example.test", Role::Member, InvitationStatus::Accepted, "2026-10-01T00:00:00.000Z"),
                invitation(16, "no@example.test", Role::Member, InvitationStatus::Rejected, "2026-10-01T00:00:00.000Z"),
            ],
        );
        let listed = listing(&all, "");
        assert_eq!(
            listed.invited.iter().map(|invitation| invitation.email.as_str()).collect::<Vec<_>>(),
            ["old@example.test", "new@example.test"]
        );
        assert_eq!(
            listed.members.iter().map(|member| member.name.as_str()).collect::<Vec<_>>(),
            ["The Owner", "Late Admin", "Zed Member", "Abe Twin", "bea twin", "Amy Member"]
        );
    }

    #[test]
    fn the_workspace_read_is_the_active_one_else_the_first() {
        let list = |active: Option<&str>| -> OrganizationsDTO {
            serde_json::from_value(serde_json::json!({
                "activeOrganizationId": active,
                "organizations": [
                    {"id": id(1), "name": "One", "slug": "one", "role": "member", "createdAt": "2026-10-01T00:00:00.000Z"},
                    {"id": id(2), "name": "Two", "slug": "two", "role": "owner", "createdAt": "2026-10-02T00:00:00.000Z"},
                ],
            }))
            .unwrap()
        };
        assert_eq!(chosen(&list(Some(&id(2)))), Some(id(2).as_str()));
        assert_eq!(chosen(&list(None)), Some(id(1).as_str()));
        // An active workspace the account is not listed in is not asked for.
        assert_eq!(chosen(&list(Some(&id(7)))), Some(id(1).as_str()));
        let empty: OrganizationsDTO =
            serde_json::from_value(serde_json::json!({"activeOrganizationId": null, "organizations": []})).unwrap();
        assert_eq!(chosen(&empty), None);
    }

    #[test]
    fn each_request_is_the_one_the_server_defines() {
        let all = details(
            Role::Owner,
            vec![member(1, "Ada Lovelace", Role::Owner, "2026-10-01T00:00:00.000Z")],
            vec![invitation(9, "ben@example.test", Role::Member, InvitationStatus::Pending, "2026-11-01T00:00:00.000Z")],
        );
        let body = serde_json::to_string(&all).unwrap();
        let fake = Fake::start(move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/api/v1/organizations") => json(
                200,
                format!(
                    r#"{{"activeOrganizationId":"{ORGANIZATION}","organizations":[{{"id":"{ORGANIZATION}","name":"Acme","slug":"acme","role":"owner","createdAt":"2026-10-01T00:00:00.000Z"}}]}}"#
                ),
            ),
            ("GET", _) => json(200, body.clone()),
            // The server's own words for an address it has no account for.
            ("POST", _) if seen.body.contains("stranger") => json(
                400,
                r#"{"error":"invalid_request","message":"stranger@example.test has no account on this server. Create it in the admin panel, then invite them."}"#,
            ),
            ("POST", _) => json(200, serde_json::to_string(&invitation(10, "x@example.test", Role::Member, InvitationStatus::Pending, "2026-11-01T00:00:00.000Z")).unwrap()),
            ("PATCH", path) if path.contains("/members/") => {
                json(200, serde_json::to_string(&member(1, "Ada Lovelace", Role::Admin, "2026-10-01T00:00:00.000Z")).unwrap())
            }
            ("PATCH", _) => json(
                200,
                format!(r#"{{"id":"{ORGANIZATION}","name":"Acme Two","slug":"acme","role":"owner","createdAt":"2026-10-01T00:00:00.000Z"}}"#),
            ),
            ("DELETE", path) if path.ends_with(&id(404)) => {
                json(403, r#"{"error":"forbidden","message":"Only the workspace owner can remove administrators or owners"}"#)
            }
            _ => json(200, r#"{"deleted":true}"#),
        });
        let client = AccountClient::for_test(fake.url.clone(), "token", None);
        let read_back = read(&client).unwrap();
        assert_eq!(read_back, all);
        rename(&client, ORGANIZATION, "Acme Two").unwrap();
        let sent = invite(
            &client,
            ORGANIZATION,
            &["cy@example.test".to_owned(), "stranger@example.test".to_owned()],
            &InviteRole::Admin,
        );
        assert_eq!(sent[0], ("cy@example.test".to_owned(), Ok(())));
        assert_eq!(
            invite_problems(&sent),
            ["stranger@example.test has no account on this server. Create it in the admin panel, then invite them."]
        );
        // One address that fails says only what the server said.
        assert_eq!(invite_problems(&sent[1..]), ["stranger@example.test has no account on this server. Create it in the admin panel, then invite them."]);
        // A failure that does not say whose it is gets the address in front, among several.
        let unreachable = [
            ("a@example.test".to_owned(), Err("The server could not be reached".to_owned())),
            ("b@example.test".to_owned(), Ok(())),
        ];
        assert_eq!(invite_problems(&unreachable), ["a@example.test: The server could not be reached"]);
        assert_eq!(invite_problems(&unreachable[..1]), ["The server could not be reached"]);
        cancel_invitation(&client, ORGANIZATION, &id(9)).unwrap();
        set_role(&client, ORGANIZATION, &id(1), InviteRole::Member).unwrap();
        remove_member(&client, ORGANIZATION, &id(1)).unwrap();
        assert_eq!(
            remove_member(&client, ORGANIZATION, &id(404)),
            Err("Only the workspace owner can remove administrators or owners".into())
        );
        // An id that is not one never reaches the server.
        assert!(remove_member(&client, ORGANIZATION, "../../notes").is_err());
        assert_eq!(
            fake.paths(),
            [
                "GET /api/v1/organizations".to_owned(),
                format!("GET /api/v1/organizations/{ORGANIZATION}"),
                format!("PATCH /api/v1/organizations/{ORGANIZATION}"),
                format!("POST /api/v1/organizations/{ORGANIZATION}/invitations"),
                format!("POST /api/v1/organizations/{ORGANIZATION}/invitations"),
                format!("DELETE /api/v1/organizations/{ORGANIZATION}/invitations/{}", id(9)),
                format!("PATCH /api/v1/organizations/{ORGANIZATION}/members/{}", id(1)),
                format!("DELETE /api/v1/organizations/{ORGANIZATION}/members/{}", id(1)),
                format!("DELETE /api/v1/organizations/{ORGANIZATION}/members/{}", id(404)),
            ]
        );
        let bodies: Vec<String> = fake.requests().into_iter().map(|seen| seen.body).filter(|body| !body.is_empty()).collect();
        assert_eq!(
            bodies,
            [
                r#"{"name":"Acme Two"}"#,
                r#"{"email":"cy@example.test","role":"admin"}"#,
                r#"{"email":"stranger@example.test","role":"admin"}"#,
                r#"{"role":"member"}"#,
            ]
        );
    }

    /// Against a running throwaway server, never a real one: it renames the
    /// workspace, changes a role, removes a member and sends an invitation.
    /// `WORKSPACE_E2E_URL`, and `WORKSPACE_E2E_TOKENS`, a directory holding
    /// the session tokens of its owner (`owner.token`) and of two members
    /// (`adam.token`, `mia.token`), with a fourth member `ned@acme.example`
    /// who is removed and invited back. Run with `--ignored`.
    #[test]
    #[ignore]
    fn e2e_the_workspace_is_managed_on_a_real_server_as_the_page_offers() {
        let url = std::env::var("WORKSPACE_E2E_URL").expect("WORKSPACE_E2E_URL");
        let tokens = std::path::PathBuf::from(std::env::var("WORKSPACE_E2E_TOKENS").expect("WORKSPACE_E2E_TOKENS"));
        let client = |who: &str| {
            let token = std::fs::read_to_string(tokens.join(format!("{who}.token"))).unwrap();
            AccountClient::for_test(url.clone(), token.trim(), None)
        };
        let (owner, adam, mia) = (client("owner"), client("adam"), client("mia"));
        let member = |details: &OrganizationDetailsDTO, email: &str| {
            details.members.iter().find(|member| member.email == email).cloned().unwrap_or_else(|| panic!("{email}"))
        };

        let first = read(&owner).unwrap();
        assert_eq!(first.organization.role, Role::Owner);
        let organization = id_in_path(&first.organization.id).unwrap();
        let organization = organization.as_str();

        // The name.
        let name = format!("Acme {}", first.organization.name.len());
        rename(&owner, organization, &name).unwrap();
        assert_eq!(read(&owner).unwrap().organization.name, name);
        assert_eq!(rename(&mia, organization, "Mine now"), Err("Workspace administrator access required".into()));

        // A role, by the owner only.
        set_role(&owner, organization, &member(&first, "adam@acme.example").id, InviteRole::Admin).unwrap();
        let second = read(&owner).unwrap();
        assert_eq!(member(&second, "adam@acme.example").role, Role::Admin);
        assert_eq!(listing(&second, "").members[0].role, Role::Owner);
        assert_eq!(listing(&second, "").members[1].email, "adam@acme.example");

        // Someone is removed, invited back, and the invitation cancelled.
        remove_member(&owner, organization, &member(&second, "ned@acme.example").id).unwrap();
        let third = read(&owner).unwrap();
        assert!(!third.members.iter().any(|member| member.email == "ned@acme.example"));
        let sent = invite(
            &owner,
            organization,
            &["ned@acme.example".to_owned(), "stranger@nowhere.example".to_owned()],
            &InviteRole::Admin,
        );
        assert_eq!(sent[0].1, Ok(()));
        assert_eq!(
            invite_problems(&sent),
            ["stranger@nowhere.example has no account on this server. Create it in the admin panel, then invite them."]
        );
        let fourth = read(&owner).unwrap();
        let invited = listing(&fourth, "ned");
        assert_eq!(invited.invited.len(), 1);
        assert_eq!(invited.invited[0].role, Role::Admin);
        let invitation = invited.invited[0].id.clone();

        // As an admin: the invitations are listed, and everything the page
        // does not offer an admin is refused by the server.
        let as_admin = read(&adam).unwrap();
        assert_eq!(as_admin.organization.role, Role::Admin);
        assert_eq!(listing(&as_admin, "").invited.len(), 1);
        assert!(invitation_actions(&Role::Admin, &Role::Admin).is_empty());
        assert!(cancel_invitation(&adam, organization, &invitation).is_err());
        let owners = as_admin.members.iter().filter(|member| member.role == Role::Owner).count();
        for target in &as_admin.members {
            let offered = member_actions(&Role::Admin, &target.role, owners);
            if !offered.contains(&Action::Remove) {
                assert!(remove_member(&adam, organization, &target.id).is_err(), "{}", target.email);
            }
            assert!(!offered.contains(&Action::MakeAdmin) && !offered.contains(&Action::MakeMember));
            if target.role != Role::Owner {
                assert_eq!(
                    set_role(&adam, organization, &target.id, InviteRole::Member),
                    Err("Only the workspace owner can change member roles".into())
                );
            }
        }
        assert_eq!(
            invite(&adam, organization, &["ned@acme.example".to_owned()], &InviteRole::Admin)[0].1,
            Err("Only the workspace owner can invite administrators".into())
        );

        // As a member: no invitations, and nothing may be changed.
        let as_member = read(&mia).unwrap();
        assert_eq!(as_member.organization.role, Role::Member);
        assert!(as_member.invitations.is_empty());
        assert_eq!(as_member.members.len(), fourth.members.len());
        for target in &as_member.members {
            assert!(member_actions(&Role::Member, &target.role, owners).is_empty());
            assert!(remove_member(&mia, organization, &target.id).is_err(), "{}", target.email);
        }
        assert!(invite(&mia, organization, &["ned@acme.example".to_owned()], &InviteRole::Member)[0].1.is_err());

        // The owner cannot be removed or demoted, as the page never offers.
        let me = member(&fourth, &first.members.iter().find(|member| member.role == Role::Owner).unwrap().email);
        assert!(member_actions(&Role::Owner, &Role::Owner, owners).is_empty());
        assert_eq!(remove_member(&owner, organization, &me.id), Err("The last workspace owner cannot be removed".into()));
        assert_eq!(
            set_role(&owner, organization, &me.id, InviteRole::Member),
            Err("The workspace owner role cannot be changed".into())
        );

        // The invitation goes, and with it the Invited group.
        cancel_invitation(&owner, organization, &invitation).unwrap();
        let last = read(&owner).unwrap();
        assert!(listing(&last, "").invited.is_empty());
        // An admin removes a member, which the page offers.
        assert_eq!(member_actions(&Role::Admin, &Role::Member, owners), [Action::Remove]);
        rename(&owner, organization, &first.organization.name).unwrap();
    }

    #[test]
    fn the_layout_stills_show_each_kind_of_viewer_a_full_table() {
        for viewer in ROLES {
            let fixture = tour_details(viewer.clone());
            assert_eq!(fixture.members.len(), 14);
            assert_eq!(fixture.members.iter().filter(|member| member.role == Role::Owner).count(), 1);
            assert_eq!(fixture.members[0].role, viewer);
            // The server shows invitations to owners and admins only.
            assert_eq!(fixture.invitations.len(), usize::from(may_manage(&viewer)));
        }
    }
}
