#!/usr/bin/env python3
"""Build a page showing the native renders at panel density. It references the
PNGs beside it, so write it into the same directory."""
import os
import sys

src, out = sys.argv[1], sys.argv[2]
names = sorted(f for f in os.listdir(src) if f.endswith(".png"))

cards = []
for f in names:
    num, _, rest = f[:-4].partition("-")
    title = f"{int(num)} · {rest.replace('-', ' ').capitalize()}"
    cards.append(f"""
<section>
  <h2>{title}</h2>
  <div class="pair">
    <figure class="native"><img src="{f}" alt="{title}, panel pixels">
      <figcaption>Panel pixels</figcaption></figure>
    <figure class="zoom"><img src="{f}" alt="{title}, enlarged 4×">
      <figcaption>Enlarged 4×</figcaption></figure>
  </div>
</section>""")

page = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Pager Native Renders</title>
<style>
:root {{ --bg:#ECECEF; --ink:#1C1C1E; --ink2:#55555B; --card:#FFFFFF; --rule:#D6D6DB; }}
@media (prefers-color-scheme: dark) {{
  :root:not([data-theme="light"]) {{ --bg:#141518; --ink:#EEF0F3; --ink2:#9AA0A8; --card:#1E2024; --rule:#2C2F35; }}
}}
:root[data-theme="dark"] {{ --bg:#141518; --ink:#EEF0F3; --ink2:#9AA0A8; --card:#1E2024; --rule:#2C2F35; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--ink);
  font:15px/1.45 -apple-system, "Helvetica Neue", Helvetica, Arial, sans-serif; }}
main {{ max-width:1180px; margin:0 auto; padding:32px 16px 64px; }}
h1 {{ font-size:28px; margin:0 0 6px; }}
p.lede {{ color:var(--ink2); margin:0 0 28px; max-width:70ch; }}
section {{ background:var(--card); border:1px solid var(--rule); border-radius:12px;
  padding:16px 18px 18px; margin:0 0 20px; }}
h2 {{ font-size:16px; margin:0 0 12px; }}
.pair {{ display:flex; gap:28px; align-items:flex-start; flex-wrap:wrap; }}
figure {{ margin:0; }}
figcaption {{ color:var(--ink2); font-size:12px; margin-top:6px; }}
figure img {{ display:block; background:#000; border-radius:3px; }}
.native img {{ width:calc(480px / var(--dpr, 1)); height:auto; }}
.zoom {{ max-width:100%; overflow-x:auto; }}
.zoom img {{ width:calc(1920px / var(--dpr, 1)); max-width:none; height:auto;
  image-rendering:pixelated; }}
</style>
</head>
<body>
<main>
  <h1>T-LoRa Pager screens at panel density</h1>
  <p class="lede">Each screen as 480×222 RGB565 pixels. The left copy puts one panel
  pixel on one physical pixel of this display, which on a Retina Mac (218–254 ppi) is
  close to the panel's roughly 227 ppi and so close to true size. The right copy
  enlarges the same pixels 4× without smoothing.</p>
  {''.join(cards)}
</main>
<script>
// Map one image pixel to one physical display pixel.
document.documentElement.style.setProperty("--dpr", window.devicePixelRatio || 1);
</script>
</body>
</html>
"""
open(out, "w").write(page)
