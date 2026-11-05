/* Recréation HTML de l'interface réelle (textes, ordre et emoji repris du code Rust).
   Illustration : les chiffres sont fictifs. */
(function () {
  const RU = (window.RU = {});
  RU.ICON = window.RU_ICON || "../site/icon-256.png";
  const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
  const lerp = (a, b, t) => a + (b - a) * t;
  const ease = (x) => (x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2);
  const noise = (i) => { const x = Math.sin(i * 12.9898 + 4.1) * 43758.5453; return x - Math.floor(x); };
  RU.noise = noise; RU.lerp = lerp; RU.ease = ease; RU.clamp = clamp;

  /* Mini-courbe : même rendu que src/chart.rs (240x36 px affichés en 120x18 pt). */
  RU.chart = (vals, lo, hi) => {
    const W = 240, H = 36, span = Math.max(1, hi - lo);
    const sample = (x) => {
      if (vals.length === 1) return vals[0];
      const pos = (x / (W - 1)) * (vals.length - 1);
      const i = Math.min(Math.floor(pos), vals.length - 2);
      const f = pos - i;
      return vals[i] * (1 - f) + vals[i + 1] * f;
    };
    const ypos = (v) => Math.round((H - 3) * (1 - clamp((v - lo) / span, 0, 1))) + 1;
    let area = `M0 ${H - 1}`, line = "";
    for (let x = 0; x < W; x++) {
      const y = ypos(sample(x));
      area += ` L${x} ${y}`;
      line += (x ? " L" : "M") + `${x} ${y + 1}`;
    }
    area += ` L${W - 1} ${H - 1} Z`;
    return `<svg class="chart" viewBox="0 0 ${W} ${H}" xmlns="http://www.w3.org/2000/svg">
      <path d="${area}" fill="rgb(90,160,255)" fill-opacity=".27"/>
      <path d="${line}" fill="none" stroke="rgb(90,160,255)" stroke-width="2"/>
      <line x1="0" x2="${W}" y1="${H - 0.5}" y2="${H - 0.5}" stroke="#8c8c8c" stroke-opacity=".35"/></svg>`;
  };

  /* Icône de la barre des menus : trois barres (comme icon.rs), en modèle monochrome. */
  RU.trayIcon = () => `<svg width="17" height="17" viewBox="0 0 36 36" fill="currentColor">
    <rect x="5" y="16" width="8" height="14" rx="1.5"/><rect x="14" y="6" width="8" height="24" rx="1.5"/><rect x="23" y="12" width="8" height="18" rx="1.5"/></svg>`;

  const wifi = `<svg width="18" height="14" viewBox="0 0 18 14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M1.5 5a11 11 0 0 1 15 0M4.2 8a7 7 0 0 1 9.6 0"/><circle cx="9" cy="11.2" r="1.3" fill="currentColor" stroke="none"/></svg>`;
  const battery = `<svg width="26" height="13" viewBox="0 0 26 13" fill="none" stroke="currentColor"><rect x=".7" y=".7" width="21.6" height="11.6" rx="3.2" opacity=".6"/><rect x="2.4" y="2.4" width="15" height="8.2" rx="1.8" fill="currentColor" stroke="none"/><rect x="23.4" y="4.3" width="1.8" height="4.4" rx=".9" fill="currentColor" stroke="none" opacity=".6"/></svg>`;

  RU.menubar = ({ title, open }) => `<div class="menubar">
    <div class="tray ${open ? "open" : ""}">${RU.trayIcon()}<span class="ttl">${esc(title)}</span></div>
    ${wifi}${battery}<span>jeu. 5 nov. 21:34</span></div>`;

  /* Menu déroulant : phrase d'état, mesures, Processus ▸, Bilan, Réglages, Quitter (menu.rs). */
  RU.menu = (m) => {
    const h = (k) => (m.hover === k ? " hover" : "");
    const chartRow = (c) => `<div class="mi">${RU.chart(c.vals, c.lo, c.hi)}<span style="opacity:.5">${esc(c.text)}</span></div>`;
    return `<div class="menu" style="top:36px;right:24px">
      <div class="mi dis">${esc(m.headline)}</div><div class="sep"></div>
      ${chartRow(m.cpu)}${chartRow(m.mem)}
      <div class="mi dis">${esc(m.disk)}</div>
      ${chartRow(m.temp)}<div class="sep"></div>
      <div class="mi${h("procs")}"><span>Processus</span><span class="arrow"><svg width="7" height="11" viewBox="0 0 7 11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M1.2 1.2l4.6 4.3-4.6 4.3"/></svg></span></div><div class="sep"></div>
      <div class="mi${h("report")}${m.busy ? " dis" : ""}"><span>${m.busy ? "Bilan en cours…" : "Bilan de santé…"}</span></div><div class="sep"></div>
      <div class="mi${h("settings")}"><span>Réglages…</span><span class="kbd">⌘,</span></div>
      <div class="mi${h("quit")}"><span>Quitter</span><span class="kbd">⌘Q</span></div></div>`;
  };

  /* Sous-menu « Processus » : seulement les lignes utiles (au plus 8) ; liste vide :
     une ligne désactivée « Aucun processus notable ». */
  RU.submenu = ({ entries, hoverIdx }) => {
    const rows = entries.slice(0, 8).map((e, i) => `<div class="mi${hoverIdx === i ? " hover" : ""}"><span>${esc(e)}</span></div>`);
    if (!rows.length) rows.push(`<div class="mi dis"><span>Aucun processus notable</span></div>`);
    // à gauche du menu principal (la barre des menus est collée au bord droit de l'écran)
    return `<div class="menu sub" style="top:188px;right:${24 + 330 + 2}px">${rows.join("")}</div>`;
  };

  /* Dialogue style osascript. */
  RU.dialog = ({ title = "Ressource Utility", text, buttons, w = 520, x, y, small, opacity = 1, scale = 1, caution }) => {
    const bs = buttons.map((b) => `<div class="btn${b.def ? " def" : ""}${b.press ? " press" : ""}">${esc(b.label)}</div>`).join("");
    return `<div class="dlg" style="width:${w}px;left:${x}px;top:${y}px;opacity:${opacity};transform:scale(${scale})">
      <div class="row"><img class="appicon" src="${RU.ICON}" alt=""/><div class="body"><h4>${esc(title)}</h4>
      <div class="txt${small ? " small" : ""}">${esc(text)}</div></div></div><div class="btns">${bs}</div></div>`;
  };

  RU.notif = ({ title, body, x, y, opacity = 1 }) => `<div class="notif" style="left:${x}px;top:${y}px;opacity:${opacity}">
    <img src="${RU.ICON}" alt=""/><div><b>${esc(title)}</b>${esc(body)}</div><span class="when">maintenant</span></div>`;

  /* Fenêtre Réglages (settings.rs, 440x420). */
  RU.settings = ({ x, y, provider = "OpenRouter (gratuit)", model, keyExists = true, local = false, status = "", autostart = true, opacity = 1 }) => {
    const A = (l, t, w, h, inner, cls = "") => `<div class="abs ${cls}" style="left:${l}px;top:${t}px;width:${w}px;height:${h}px">${inner}</div>`;
    const row = (t, label, field) => A(20, t + 4, 115, 20, `<span class="lbl">${label}</span>`) + A(140, t, 280, 26, field);
    const keyRow = local
      ? row(206, "Adresse du serveur", `<div class="field">http://localhost:1234/v1</div>`)
      : row(206, "Clé API", `<div class="field"><span class="ph">${keyExists ? "•••• enregistrée dans le Trousseau" : "Colle ta clé API ici"}</span></div>`);
    const sep = `<div class="abs" style="left:20px;top:76px;width:400px;height:1px;background:rgba(128,128,128,.35)"></div>`;
    return `<div class="win" style="left:${x}px;top:${y}px;opacity:${opacity}">
      <div class="tb"><div class="lights"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i></div>Réglages — Ressource Utility</div>
      <div class="cnt">
        ${A(20, 16, 400, 18, `<span class="h">Général</span>`)}
        ${A(40, 42, 380, 20, `<span class="check">${autostart ? "✓" : ""}</span>Lancer au démarrage`)}
        ${sep}
        ${A(20, 90, 400, 18, `<span class="h">Intelligence artificielle</span>`)}
        ${row(122, "Fournisseur", `<div class="field pop"><span>${esc(provider)}</span><span class="chev"><svg width="9" height="13" viewBox="0 0 9 13" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M1.5 4.8L4.5 1.8l3 3M1.5 8.2l3 3 3-3"/></svg></span></div>`)}
        ${row(164, "Modèle", `<div class="field pop" style="justify-content:space-between"><span>${esc(model)}</span><span class="chev"><svg width="10" height="7" viewBox="0 0 10 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M1.2 1.4l3.8 3.8 3.8-3.8"/></svg></span></div>`)}
        ${keyRow}
        ${local ? A(140, 238, 280, 18, `<span class="hintt">100 % local, rien ne sort du Mac</span>`) : A(140, 238, 150, 18, `<span class="hintt">Dans le Trousseau macOS</span>`) + (keyExists ? A(300, 234, 125, 26, `Supprimer la clé`, "pbtn") : "")}
        ${A(140, 266, 285, 20, `<span class="link">Voir ce qui est envoyé à l'IA…</span>`)}
        ${A(20, 298, 400, 56, esc(status))}
        ${A(20, 372, 95, 32, `Tester`, "pbtn")}${A(230, 372, 95, 32, `Annuler`, "pbtn")}${A(330, 372, 95, 32, `Enregistrer`, "pbtn def")}
      </div></div>`;
  };

  /* ---- Données fictives communes ---- */
  RU.series = (base, amp, n = 60, seed = 1) => Array.from({ length: n }, (_, i) =>
    clamp(base + amp * (0.65 * Math.sin(i / (5 + seed) + seed * 2) + 0.35 * (noise(i + seed * 50) - 0.5) * 2), 1, 99));
  RU.PROCS = [
    "✅ WindowServer — 9 % CPU — 310 Mo",
    "👀 Google Chrome Helper ×18 — 7 % CPU — 2,9 Go",
    "✅ Claude Helper ×6 — 4 % CPU — 1,1 Go",
    "✅ mdworker_shared ×8 — 3 % CPU — 240 Mo",
    "❓ syncagentd — 2 % CPU — 85 Mo",
  ];
  RU.REPORT = `État du Mac
⚠️ Disque presque plein : 14 Go libres (6 %)
✅ CPU 21 % · Mémoire 50 % · 48 °C — normal

Sécurité
✅ Aucun logiciel suspect détecté

Ce qui tourne
✅ WindowServer — Normal
   Dessine tout ce qui s'affiche à l'écran. Il monte avec plusieurs écrans ou beaucoup de fenêtres.
👀 Google Chrome Helper ×18 — À surveiller
   Un onglet ou une extension de Google Chrome. Normal, mais beaucoup de RAM = trop d'onglets ouverts. Il utilise 2,9 Go de RAM : ferme ce qui ne sert pas.
✅ Claude Helper ×6 — Normal
   Partie de l'application Claude (processus de fond). Normal.
✅ mdworker_shared ×8 — Normal
   Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.
❓ syncagentd — Inconnu
   Inconnu — configure une clé IA pour l'identifier.

👉 Conseil : Libère de l'espace disque (Réglages > Général > Stockage).`;
  RU.CHROME_DLG = `👀 Google Chrome Helper — À surveiller

Un onglet ou une extension de Google Chrome. Normal, mais beaucoup de RAM = trop d'onglets ouverts. Il utilise 2,9 Go de RAM : ferme ce qui ne sert pas.

CPU 7 % · mémoire 2,9 Go`;
  RU.XMRIG_DLG = `🛑 xmrig — Suspect

ressemble à un mineur de cryptomonnaie : CPU 95 % depuis 4 min, nom connu de mineur de cryptomonnaie (xmrig), lancé depuis un dossier temporaire (/tmp), non signé

Programme : /tmp/xmrig`;

  const staticMenu = (hover) => ({
    headline: "⚠️ Disque presque plein — 14 Go libres",
    cpu: { text: "CPU 21 %", vals: RU.series(22, 17, 60, 1), lo: 0, hi: 100 },
    mem: { text: "Mémoire 50 % · 8,1 / 16 Go", vals: RU.series(50, 5, 60, 2), lo: 0, hi: 100 },
    disk: "Disque 14 Go libres (6 %)",
    temp: { text: "Température 48 °C", vals: RU.series(48, 8, 60, 3), lo: 20, hi: 100 },
    hover,
  });
  const TITLE = "⚠️ 21% · 50% · 14G";

  /* ---- Scènes statiques ---- */
  RU.scenes = {
    menu: (t) => ({ w: 820, h: 500, html: RU.menubar({ title: TITLE, open: true }) + RU.menu(staticMenu("")) }),
    processus: (t) => ({
      w: 880, h: 500,
      html: RU.menubar({ title: TITLE, open: true }) + RU.menu(staticMenu("procs")) + RU.submenu({ entries: RU.PROCS, hoverIdx: 1 }),
    }),
    "processus-dialog": (t) => ({
      w: 780, h: 390,
      html: RU.dialog({ text: RU.CHROME_DLG, buttons: [{ label: "Afficher dans le Finder" }, { label: "Forcer à quitter…" }, { label: "OK", def: 1 }], w: 600, x: 90, y: 70 }),
    }),
    bilan: (t) => ({
      w: 860, h: 610,
      html: RU.dialog({ text: RU.REPORT, small: 1, buttons: [{ label: "Actualiser" }, { label: "OK", def: 1 }], w: 700, x: 80, y: 40 }),
    }),
    reglages: (t) => ({
      w: 640, h: 560,
      html: RU.settings({ x: 100, y: 60, provider: "OpenRouter (gratuit)", model: "google/gemma-4-31b-it:free", status: "✅ Connexion OK (modèle google/gemma-4-31b-it:free)" }),
    }),
  };
  RU.render = (scene, theme) => {
    const s = RU.scenes[scene](theme);
    return { w: s.w, h: s.h, html: `<div class="desk theme-${theme}" style="width:${s.w}px;height:${s.h}px">${s.html}<div class="tag">Illustration</div></div>` };
  };
})();
