#!/usr/bin/env python3
"""Design source for the Cereal logo: a bitten cereal loop that reads as a C,
bobbing in milk. Original artwork; no third-party assets.

Writes the vector masters. Rasterising them (PNG/ICNS/iOS icon set) is done by
`scripts/generate-app-icons.sh`, which needs nothing beyond macOS.

Only needed when changing the artwork:
    python3 -m venv .venv && .venv/bin/pip install shapely
    .venv/bin/python scripts/logo/cereal_logo.py
"""
import math
from pathlib import Path

from shapely import affinity
from shapely.geometry import Point, Polygon
from shapely.ops import unary_union

ROOT = Path(__file__).resolve().parents[2]
DESKTOP_ICONS = ROOT / "apps/desktop/assets/app-icons"
DESKTOP_MARK = ROOT / "apps/desktop/assets/icons/cereal.svg"
# The web app serves the mark from `public` and inlines the same file from
# `src/assets`; its app icon is the default Dock icon.
WEB_MARKS = (ROOT / "apps/web/public/mark.svg", ROOT / "apps/web/src/assets/mark.svg")
WEB_APP_ICON = ROOT / "apps/web/src/assets/app-icon.svg"
IOS_MASTERS = ROOT / "scripts/logo/ios"

# Colourways. `gold`, `midnight` and `paper` are the Dock icon choices; `bronze`
# and `tinted` are the iOS dark and tinted appearances. The golds are the
# counterparts of the lime the brand had before, from scripts/logo/gold_palette.py
# (same lightness and chroma, hue moved to gold): do not adjust them by eye.
# The dark ink of the loop and the ground of the iOS dark icon were a forest
# green (hue 156 to 159; hex 2B3F32, 19251E and 2A3D31). They follow the same
# rule, lightness and chroma kept and hue 82: #423825, #272116 and #403624,
# a bronze-black.
SCHEMES = {
    "gold": dict(bg=("#F4C247", "#D5A413"), loop=("#423825", "#272116"), milk=("#FBF6E6", "#EDE5CB"), pit="#F3DEB1", rim="#FFFDF4"),
    "midnight": dict(bg=("#2F4B5C", "#1D303C"), loop=("#FAE9C3", "#E7CE98"), milk=("#FBF6E6", "#E6DFC8"), pit="#FFFFFF", rim="#FFFFFF"),
    "paper": dict(bg=("#F7F1DF", "#E6DBBE"), loop=("#9C4E36", "#783726"), milk=("#FFFFFF", "#F8F3E6"), pit="#F4C9A8", rim="#FFFFFF"),
    "bronze": dict(bg=("#403624", "#272116"), loop=("#F7C448", "#D8A617"), milk=("#FBF6E6", "#EDE5CB"), pit="#FFFFFF", rim="#FFFDF4"),
    "tinted": dict(bg=("#1C1C1C", "#0A0A0A"), loop=("#F2F2F2", "#CFCFCF"), milk=("#8C8C8C", "#6E6E6E"), pit="#FFFFFF", rim="#B4B4B4"),
}

# Geometry lives in the coordinates of the macOS icon plate: 824 px at (100, 100)
# on a 1024 px canvas. Full-bleed (iOS) output scales the same art to the edges.
CX, CY, R_OUT, R_IN = 498, 458, 240, 96
BITE = -24  # direction of the bite, degrees (SVG y points down)
TEETH = [(274, -27, 104), (206, 3, 114), (278, 30, 84)]  # distance, angle offset, radius @ R=250
CRUMBS = [(302, -3, 25), (352, -17, 14), (322, 19, 12)]
PITS = [(-150, -95, 9, 6, 30), (-178, 20, 7, 5, -20), (-60, -178, 8, 5, 60), (-110, 130, 9, 6, 10),
        (40, 168, 7, 5, -40), (150, 110, 8, 5, 30)]
DROPLETS = [(190, 560, 17), (222, 520, 8)]
SEGMENTS = 96


def circle(cx, cy, radius):
    return Point(cx, cy).buffer(radius, resolution=SEGMENTS)


def ellipse(cx, cy, rx, ry, rotation):
    shape = affinity.rotate(affinity.scale(circle(0, 0, 1), rx, ry), rotation, origin=(0, 0))
    return affinity.translate(shape, cx, cy)


def polar(cx, cy, distance, degrees):
    angle = math.radians(degrees)
    return cx + distance * math.cos(angle), cy + distance * math.sin(angle)


def soften(shape, convex=0.0, concave=0.0):
    """Round convex corners (morphological opening) and concave ones (closing)."""
    if convex:
        shape = shape.buffer(-convex, resolution=SEGMENTS).buffer(convex, resolution=SEGMENTS)
    if concave:
        shape = shape.buffer(concave, resolution=SEGMENTS).buffer(-concave, resolution=SEGMENTS)
    return shape


def loop_shape(cx=CX, cy=CY, r_out=R_OUT, r_in=R_IN):
    k = r_out / 250
    bite = unary_union([circle(*polar(cx, cy, d * k, BITE + a), radius * k) for d, a, radius in TEETH])
    loop = circle(cx, cy, r_out).difference(circle(cx, cy, r_in)).difference(bite)
    return soften(loop, convex=9 * k, concave=4.5 * k)


def crumb_shape(cx=CX, cy=CY, r_out=R_OUT, specs=CRUMBS):
    k = r_out / 250
    return unary_union([circle(*polar(cx, cy, d * k, BITE + a), size * k) for d, a, size in specs])


def milk_surface(x):
    u = (x - 100) / 824
    y = 642 - 24 * math.sin(2 * math.pi * (u * 0.95 + 0.10)) - 9 * math.sin(2 * math.pi * (u * 2.2 + 0.33))
    return y - 26 * math.exp(-(((x - 196) / 40) ** 2))  # swell under the droplets


def milk_shape():
    xs = [i * 3 for i in range(-10, 360)]
    return Polygon([(x, milk_surface(x)) for x in xs] + [(xs[-1], 1100), (xs[0], 1100)])


def number(value, precision=1):
    return f"{value:.{precision}f}".rstrip("0").rstrip(".")


def path_data(shape, precision=1, tolerance=0.12):
    shape = shape.simplify(tolerance, preserve_topology=True)
    polygons = list(shape.geoms) if hasattr(shape, "geoms") else [shape]
    parts = []
    for polygon in polygons:
        if polygon.is_empty or not isinstance(polygon, Polygon):
            continue  # booleans can leave stray points and lines behind
        for ring in [polygon.exterior, *polygon.interiors]:
            points = list(ring.coords)[:-1]
            parts.append("M" + " ".join(f"{number(x, precision)},{number(y, precision)}" for x, y in points) + "Z")
    return "".join(parts)


def squircle(x, y, width, height, radius):
    """Continuous-corner rounded rectangle (the macOS/iOS icon silhouette)."""
    a, b, c, d, e, f, g = 1.528665, 1.08849296, 0.86840694, 0.63149379, 0.37282383, 0.16905956, 0.07491139
    right, bottom, r = x + width, y + height, radius
    p = lambda px, py: f"{number(px, 2)},{number(py, 2)}"
    return (
        f"M{p(x + a*r, y)}L{p(right - a*r, y)}"
        f"C{p(right - b*r, y)} {p(right - c*r, y)} {p(right - d*r, y + g*r)}"
        f"C{p(right - e*r, y + f*r)} {p(right - f*r, y + e*r)} {p(right - g*r, y + d*r)}"
        f"C{p(right, y + c*r)} {p(right, y + b*r)} {p(right, y + a*r)}L{p(right, bottom - a*r)}"
        f"C{p(right, bottom - b*r)} {p(right, bottom - c*r)} {p(right - g*r, bottom - d*r)}"
        f"C{p(right - f*r, bottom - e*r)} {p(right - e*r, bottom - f*r)} {p(right - d*r, bottom - g*r)}"
        f"C{p(right - c*r, bottom)} {p(right - b*r, bottom)} {p(right - a*r, bottom)}L{p(x + a*r, bottom)}"
        f"C{p(x + b*r, bottom)} {p(x + c*r, bottom)} {p(x + d*r, bottom - g*r)}"
        f"C{p(x + e*r, bottom - f*r)} {p(x + f*r, bottom - e*r)} {p(x + g*r, bottom - d*r)}"
        f"C{p(x, bottom - c*r)} {p(x, bottom - b*r)} {p(x, bottom - a*r)}L{p(x, y + a*r)}"
        f"C{p(x, y + b*r)} {p(x, y + c*r)} {p(x + g*r, y + d*r)}"
        f"C{p(x + f*r, y + e*r)} {p(x + e*r, y + f*r)} {p(x + d*r, y + g*r)}"
        f"C{p(x + c*r, y)} {p(x + b*r, y)} {p(x + a*r, y)}Z"
    )


PLATE = squircle(100, 100, 824, 824, 185.4)


def icon_svg(scheme, full_bleed=False):
    colours = SCHEMES[scheme]
    loop, crumbs, milk = loop_shape(), crumb_shape(), milk_shape()
    submerged = loop.intersection(milk)
    # Milk creeps up the loop where they meet, then droplets pinch off the swell.
    hugged = soften(unary_union([milk, loop]), concave=26)
    milk = unary_union([milk, hugged.difference(unary_union([milk, loop]))])
    milk = unary_union([milk] + [circle(*droplet) for droplet in DROPLETS])
    surface = milk.difference(affinity.translate(milk, 0, 9)).difference(loop.buffer(2))
    dry = unary_union([loop, crumbs]).difference(submerged)
    pits = unary_union([ellipse(CX + x, CY + y, rx, ry, rot) for x, y, rx, ry, rot in PITS])
    pits = pits.intersection(loop.buffer(-14)).difference(milk)

    if full_bleed:
        plate = '<rect width="1024" height="1024" fill="url(#bg)"/>'
        art = f'<g transform="translate(512 512) scale({number(1024 / 824, 4)}) translate(-512 -512)">'
        clip = '<rect width="1024" height="1024"/>'
    else:
        plate = f'<path d="{PLATE}" fill="url(#bg)"/>'
        art = "<g>"
        clip = f'<path d="{PLATE}"/>'
    gradient = lambda name, x1, y1, x2, y2, stops: (
        f'<linearGradient id="{name}" gradientUnits="userSpaceOnUse" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">'
        f'<stop offset="0" stop-color="{stops[0]}"/><stop offset="1" stop-color="{stops[1]}"/></linearGradient>'
    )
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>'
        + gradient("bg", 0, 0 if full_bleed else 100, 0, 1024 if full_bleed else 924, colours["bg"])
        + gradient("loop", CX - R_OUT, CY - R_OUT, CX + R_OUT, CY + R_OUT, colours["loop"])
        + gradient("milk", 0, 610, 0, 924, colours["milk"])
        + f'<clipPath id="plate">{clip}</clipPath></defs>{plate}<g clip-path="url(#plate)">{art}'
        + f'<path d="{path_data(dry)}" fill="url(#loop)" fill-rule="evenodd"/>'
        + f'<path d="{path_data(pits)}" fill="{colours["pit"]}" fill-opacity="0.2"/>'
        + f'<path d="{path_data(milk)}" fill="url(#milk)" fill-rule="evenodd"/>'
        + f'<path d="{path_data(submerged)}" fill="{colours["loop"][1]}" fill-opacity="0.13" fill-rule="evenodd"/>'
        + f'<path d="{path_data(surface)}" fill="{colours["rim"]}" fill-opacity="0.9" fill-rule="evenodd"/>'
        + "</g></g></svg>\n"
    )


def mark_svg():
    """Single-colour glyph on a 24-unit grid; the colour comes from the host."""
    shape = unary_union([loop_shape(512, 512, 250, 100), crumb_shape(512, 512, 250, [(304, -3, 28), (360, -19, 17)])])
    min_x, min_y, max_x, max_y = shape.bounds
    scale = 22.6 / max(max_x - min_x, max_y - min_y)
    shape = affinity.scale(shape, scale, scale, origin=(0, 0))
    shape = affinity.translate(shape, 12 - (min_x + max_x) / 2 * scale, 12 - (min_y + max_y) / 2 * scale)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="currentColor" fill-rule="evenodd" '
        f'd="{path_data(shape, precision=3, tolerance=0.004)}"/></svg>\n'
    )


def main():
    for scheme in ("gold", "midnight", "paper"):
        (DESKTOP_ICONS / f"cereal-{scheme}.svg").write_text(icon_svg(scheme))
    WEB_APP_ICON.write_text(icon_svg("gold"))
    IOS_MASTERS.mkdir(parents=True, exist_ok=True)
    for name, scheme in (("AppIcon", "gold"), ("AppIcon-Dark", "bronze"), ("AppIcon-Tinted", "tinted")):
        (IOS_MASTERS / f"{name}.svg").write_text(icon_svg(scheme, full_bleed=True))
    mark = mark_svg()
    for path in (DESKTOP_MARK, *WEB_MARKS):
        path.write_text(mark)


if __name__ == "__main__":
    main()
