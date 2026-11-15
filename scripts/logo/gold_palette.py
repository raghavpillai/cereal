#!/usr/bin/env python3
"""Cereal's gold palette: turns one of the old lime/olive brand colours into its gold counterpart.

    python3 scripts/logo/gold_palette.py b4c342 5b6f00 '#74bd00'      # prints old -> new, one per line
    python3 scripts/logo/gold_palette.py --rgb 0.706 0.765 0.259      # sRGB components 0..1 in, same out
    python3 scripts/logo/gold_palette.py --scan apps/web/src ...      # lists every brand green left in text files

--scan reads hex colours written as #rrggbb, #rrggbbaa, #rgb, 0xrrggbb and 0xrrggbbaa. It does not see
rgb()/hsl() functions, float components, named colours or bitmaps: check those by hand.

The rule works in OKLCH and keeps each colour's lightness and chroma, so every colour keeps the role
and the contrast it had; only the hue moves, from lime (about 116 degrees) to gold:
  - a colour is a brand green when its chroma is at least 0.012 and its hue is between 108 and 170
    (limes and olives sit near 116, the forest inks and green-black surfaces near 155);
    anything else (the warm paper neutrals, greys, reds, blues, the yellow highlighter) is returned unchanged;
  - light and mid tones (lightness 0.66 and up) go to hue 86, a true gold;
  - dark inks (lightness 0.58 and below) go to hue 82, so they read as bronze rather than olive;
  - in between, the hue is interpolated;
  - chroma is reduced only as far as needed to stay inside sRGB;
  - two greens belong to other companies' logos and are never changed or reported: Google's
    #34A853 and Microsoft's #7FBA00 (the sign-in buttons draw those logos).
"""
import math, sys

def srgb_to_lin(c): return c/12.92 if c<=0.04045 else ((c+0.055)/1.055)**2.4
def lin_to_srgb(c): return 12.92*c if c<=0.0031308 else 1.055*(c**(1/2.4))-0.055
def hex_to_oklch(h):
    r,g,b=[srgb_to_lin(int(h[i:i+2],16)/255) for i in (0,2,4)]
    l=0.4122214708*r+0.5363325363*g+0.0514459929*b; m=0.2119034982*r+0.6806995451*g+0.1073969566*b; s=0.0883024619*r+0.2817188376*g+0.6299787005*b
    l,m,s=[x**(1/3) for x in (l,m,s)]
    L=0.2104542553*l+0.7936177850*m-0.0040720468*s; a=1.9779984951*l-2.4285922050*m+0.4505937099*s; b2=0.0259040371*l+0.7827717662*m-0.8086757660*s
    return L,math.hypot(a,b2),math.degrees(math.atan2(b2,a))%360
def oklch_to_lin(L,C,H):
    a=C*math.cos(math.radians(H)); b=C*math.sin(math.radians(H))
    l=(L+0.3963377774*a+0.2158037573*b)**3; m=(L-0.1055613458*a-0.0638541728*b)**3; s=(L-0.0894841775*a-1.2914855480*b)**3
    return (4.0767416621*l-3.3077115913*m+0.2309699292*s, -1.2684380046*l+2.6097574011*m-0.3413193965*s, -0.0041960863*l-0.7034186147*m+1.7076147010*s)
def in_gamut(rgb): return all(-1e-4<=c<=1+1e-4 for c in rgb)
def oklch_to_hex(L,C,H):
    # reduce chroma until in gamut (keeps lightness and hue)
    lo,hi=0,C
    if in_gamut(oklch_to_lin(L,C,H)): lo=C
    else:
        for _ in range(30):
            mid=(lo+hi)/2
            if in_gamut(oklch_to_lin(L,mid,H)): lo=mid
            else: hi=mid
    rgb=oklch_to_lin(L,lo,H)
    return ''.join('%02x'%round(255*min(1,max(0,lin_to_srgb(min(1,max(0,c)))))) for c in rgb)
def lum(h):
    r,g,b=[srgb_to_lin(int(h[i:i+2],16)/255) for i in (0,2,4)]; return 0.2126*r+0.7152*g+0.0722*b
def contrast(a,b):
    x,y=sorted((lum(a),lum(b)),reverse=True); return (x+0.05)/(y+0.05)

# Greens inside the window that are part of someone else's logo, not of our brand.
THIRD_PARTY = {"34a853": "Google", "7fba00": "Microsoft"}

def is_brand_green(hex6):
    if hex6.lower() in THIRD_PARTY: return False
    L, C, H = hex_to_oklch(hex6)
    return C >= 0.012 and 108 <= H <= 170

def gold(hex6):
    """The gold counterpart of a six-digit sRGB hex colour (no '#'), lower case."""
    hex6 = hex6.lower()
    L, C, H = hex_to_oklch(hex6)
    if not is_brand_green(hex6): return hex6
    hue = 82 if L <= 0.58 else 86 if L >= 0.66 else 82 + (L - 0.58) / 0.08 * 4
    return oklch_to_hex(L, C, hue)

if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["--scan"]:
        import os, re
        pattern = re.compile(r'(?:#|0x)([0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3})(?![0-9a-fA-F])')
        skip = {'node_modules', '.git', 'target', 'build', 'dist', '.output', '.build', 'generated', 'DerivedData', '.swiftpm'}
        found = 0
        for root in args[1:]:
            walk = os.walk(root) if os.path.isdir(root) else [(os.path.dirname(root), [], [os.path.basename(root)])]
            for folder, folders, files in walk:
                folders[:] = [f for f in folders if f not in skip]
                for name in files:
                    path = os.path.join(folder, name)
                    try: text = open(path, encoding='utf8').read()
                    except (UnicodeDecodeError, OSError): continue
                    if os.path.abspath(path) == os.path.abspath(__file__): continue
                    for number, line in enumerate(text.split('\n'), 1):
                        for match in pattern.finditer(line):
                            value = match.group(1).lower()
                            if len(value) == 3: value = ''.join(c * 2 for c in value)
                            if is_brand_green(value[:6]):
                                found += 1
                                print(f"{path}:{number}: #{match.group(1)} -> #{gold(value[:6])}{value[6:]}")
        print(f"{found} brand green(s) left")
        sys.exit(1 if found else 0)
    elif args[:1] == ["--rgb"]:
        new = gold(''.join('%02x' % round(float(c) * 255) for c in args[1:4]))
        print(' '.join('%.3f' % (int(new[i:i+2], 16) / 255) for i in (0, 2, 4)), '#' + new)
    else:
        for arg in args:
            old = arg.lstrip('#').lower().removeprefix('0x')
            new = gold(old)
            print(f"#{old} -> #{new}" + ("  (unchanged: not a brand green)" if new == old else f"  contrast on #f7f7f2 {contrast(new, 'f7f7f2'):.2f}, on #1e1e1e {contrast(new, '1e1e1e'):.2f}"))
