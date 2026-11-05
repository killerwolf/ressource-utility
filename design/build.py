#!/usr/bin/env python3
"""Assemble site/demo.html (autonome, sans dépendance externe) à partir de design/."""
import base64, pathlib, subprocess
here = pathlib.Path(__file__).resolve().parent
site = here.parent / "site"
icon = subprocess.run(["sips", "-z", "128", "128", str(site / "icon.png"), "--out", "/tmp/ru-icon128.png"], capture_output=True)
data = base64.b64encode(pathlib.Path("/tmp/ru-icon128.png").read_bytes()).decode()
page = (here / "demo.template.html").read_text()
page = page.replace("/*__CSS__*/", (here / "ui.css").read_text())
page = page.replace("__ICON__", "data:image/png;base64," + data)
page = page.replace("/*__UI__*/", (here / "ui.js").read_text())
page = page.replace("/*__DEMO__*/", (here / "demo.js").read_text())
(site / "demo.html").write_text(page)
print("site/demo.html", len(page) // 1024, "ko")
