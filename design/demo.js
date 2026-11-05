/* Démo animée : tout est une fonction pure du temps t (secondes) → déterministe,
   donc enregistrable image par image. Illustration : chiffres fictifs. */
(function () {
  // Sortie 1280x800 ; l'interface est dessinée en 984x616 puis agrandie (texte lisible en GIF).
  const OUT_W = 1280, OUT_H = 800, W = 984, H = 616, S = OUT_W / W, DUR = 25;
  const { lerp, ease, clamp, noise } = RU;
  const seg = (t, a, b) => ease(clamp((t - a) / (b - a), 0, 1));
  const inR = (t, a, b) => t >= a && t < b;
  const oneLine = (s, n) => { const f = s.split(/\s+/).join(" "); return f.length <= n ? f : f.slice(0, n).trimEnd() + "…"; };

  const SUS0 = 4.0, SUS1 = 15.6;
  const cpu = (t) => {
    if (t < 2) return 13 + 3 * noise(Math.floor(t * 3));
    if (t < 4.2) return lerp(14, 96, seg(t, 2, 4.2));
    if (t < 14.8) return 95 + 2 * noise(Math.floor(t * 4) + 9);
    if (t < 16.4) return lerp(96, 12, seg(t, 14.8, 16.4));
    return 12 + 3 * noise(Math.floor(t * 3) + 3);
  };
  const temp = (t) => 46 + (cpu(t) - 13) * 0.3;
  const mem = (t) => 48 + (t > 4 && t < 15.6 ? 1.5 : 0) + noise(Math.floor(t * 2)) * 0.6;
  const suspect = (t) => inR(t, SUS0, SUS1);

  const PROCS = [
    "✅ WindowServer — 9 % CPU — 310 Mo",
    "👀 Google Chrome Helper ×18 — 7 % CPU — 2,9 Go",
    "✅ Claude Helper ×6 — 4 % CPU — 1,1 Go",
    "✅ mdworker_shared ×8 — 3 % CPU — 240 Mo",
    "❓ syncagentd — 2 % CPU — 85 Mo",
  ];
  const REASONS = "CPU 96 % depuis 1 min, nom connu de mineur de cryptomonnaie (xmrig), lancé depuis un dossier temporaire (/tmp), non signé";
  const XMRIG_DLG = `🛑 xmrig — Suspect\n\nressemble à un mineur de cryptomonnaie : ${REASONS}\n\nProgramme : /tmp/xmrig`;
  const CONFIRM = `Forcer « xmrig » (PID 4821) à quitter ?\n\nressemble à un mineur de cryptomonnaie : ${REASONS}`;
  const REPORT_CALM = `État du Mac
✅ CPU 12 % · Mémoire 49 % · Disque 42 Go libres · 46 °C — normal

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
   Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.`;

  const MENU_X = W - 24 - 330;           // bord gauche du menu
  const SUB_X = MENU_X - 2 - 392;        // bord gauche du sous-menu
  const dlgBtns = (def, press) => (labels) => labels.map((l) => ({ label: l, def: l === def, press: press === l }));

  /* ---- Contenu de l'écran à l'instant t (sans le curseur) ---- */
  function scene(t) {
    const sus = suspect(t);
    const c = Math.round(cpu(t)), m = Math.round(mem(t));
    const title = `${sus ? "🛑 " : ""}${c}% · ${m}% · 42G`;
    const open1 = inR(t, 6.95, 9.75), open2 = inR(t, 17.15, 18.5);
    const open = open1 || open2;
    let hover = "";
    if (open1 && t >= 7.7) hover = "procs";
    if (open2 && t >= 17.9) hover = "report";
    const hist = (f, lo) => Array.from({ length: 60 }, (_, k) => f(Math.max(0, t - (59 - k) * 0.42)) + (noise(k * 7) - 0.5) * 3);
    let html = RU.menubar({ title, open });
    if (open) {
      html += RU.menu({
        headline: sus ? "🛑 Logiciel suspect : xmrig" : "✅ Tout va bien",
        cpu: { text: `CPU ${c} %`, vals: hist(cpu), lo: 0, hi: 100 },
        mem: { text: `Mémoire ${m} % · ${(m * 0.16).toFixed(1).replace(".", ",")} / 16 Go`, vals: hist(mem), lo: 0, hi: 100 },
        disk: "Disque 42 Go libres (17 %)",
        temp: { text: `Température ${Math.round(temp(t))} °C`, vals: hist(temp), lo: 20, hi: 100 },
        hover,
      });
      if (open1 && t >= 8.3) {
        const entries = sus ? ["🛑 xmrig — Suspect", ...PROCS] : PROCS;
        html += RU.submenu({ entries, hoverIdx: t >= 9.0 ? 0 : -1 });
      }
    }
    // Dialogues
    if (inR(t, 9.9, 12.85)) {
      const a = seg(t, 9.9, 10.15);
      html += RU.dialog({ text: XMRIG_DLG, w: 600, x: (W - 600) / 2, y: 150, opacity: a, scale: 0.96 + 0.04 * a,
        buttons: dlgBtns("OK", t > 12.62 ? "Forcer à quitter…" : "")(["Afficher dans le Finder", "Forcer à quitter…", "OK"]) });
    }
    if (inR(t, 12.9, 14.65)) {
      const a = seg(t, 12.9, 13.15);
      html += RU.dialog({ text: CONFIRM, w: 560, x: (W - 560) / 2, y: 180, opacity: a, scale: 0.96 + 0.04 * a,
        buttons: dlgBtns("Annuler", t > 14.45 ? "Forcer à quitter" : "")(["Annuler", "Forcer à quitter"]) });
    }
    if (inR(t, 18.6, 21.45)) {
      const a = seg(t, 18.6, 18.85);
      html += RU.dialog({ text: REPORT_CALM, small: 1, w: 700, x: (W - 700) / 2, y: 70, opacity: a, scale: 0.96 + 0.04 * a,
        buttons: dlgBtns("OK", t > 21.2 ? "OK" : "")(["Actualiser", "OK"]) });
    }
    if (t >= 21.9) {
      const a = seg(t, 21.9, 22.2);
      html += RU.settings({ x: (W - 440) / 2, y: 80, provider: "LM Studio (local)", model: "qwen3-4b-instruct", local: true, autostart: true,
        opacity: a, status: t > 23.6 ? "✅ Connexion OK (modèle qwen3-4b-instruct)" : "" });
    }
    // Notifications
    const nIn = (a, b) => { const s = seg(t, a, a + 0.35) * (1 - seg(t, b - 0.35, b)); return s; };
    if (inR(t, 4.3, 7.4)) {
      const s = nIn(4.3, 7.4);
      html += RU.notif({ title: "Ressource Utility", body: "🛑 Logiciel suspect : xmrig — " + oneLine(`ressemble à un mineur de cryptomonnaie : ${REASONS} /tmp/xmrig`, 140), x: W - 360 - 14 + (1 - s) * 380, y: 44, opacity: s });
    }
    if (inR(t, 14.7, 17.0)) {
      const s = nIn(14.7, 17.0);
      html += RU.notif({ title: "Ressource Utility", body: "xmrig a été arrêté.", x: W - 360 - 14 + (1 - s) * 380, y: 44, opacity: s });
    }
    return html;
  }

  const caption = (t) => {
    if (t < 4.0) return "Un Mac calme : ✅ Tout va bien";
    if (t < 6.9) return "Un processus s'emballe : la barre des menus passe en 🛑";
    if (t < 9.9) return "La liste Processus met les suspects en premier";
    if (t < 12.9) return "Le dialogue explique pourquoi, en français";
    if (t < 14.8) return "Arrêt du processus, après confirmation";
    if (t < 18.6) return "De retour au calme";
    if (t < 21.9) return "Bilan de santé : un verdict par processus";
    return "Réglages : IA au choix, dont LM Studio 100 % local";
  };

  /* ---- Curseur : positions des cibles mesurées dans le DOM ---- */
  let POS = null;
  function measure() {
    const probe = document.createElement("div");
    probe.className = "desk theme-dark";
    probe.style.cssText = `width:${W}px;height:${H}px;position:absolute;left:0;top:0;visibility:hidden;transform:scale(${S});transform-origin:0 0`;
    document.body.appendChild(probe);
    const find = (t, text, nth = 0) => {
      probe.innerHTML = scene(t);
      const els = [...probe.querySelectorAll(".mi, .btn")].filter((e) => e.textContent.trim().includes(text));
      const e = els[nth];
      if (!e) return { x: 500, y: 300 };
      const r = e.getBoundingClientRect();
      return { x: (r.left + Math.min(r.width / 2, 60 * S)) / S, y: (r.top + r.height / 2) / S };
    };
    POS = {
      tray: { x: W - 203 - 70, y: 16 },
      proc: find(8.0, "Processus"),
      xmrig: find(9.2, "xmrig — Suspect"),
      kill: find(11.0, "Forcer à quitter…"),
      confirm: find(13.5, "Forcer à quitter"),
      bilan: find(17.5, "Bilan de santé"),
      ok: find(20.0, "OK"),
      cancel: { x: (W - 440) / 2 + 277, y: 80 + 38 + 388 },
      idleA: { x: 500, y: 400 }, idleB: { x: 590, y: 460 },
    };
    probe.remove();
  }
  const keys = () => [
    [0, "idleA"], [5.8, "idleA"], [6.9, "tray"], [7.7, "proc"], [9.0, "xmrig"], [9.7, "xmrig"],
    [10.5, "idleA"], [11.6, "kill"], [12.7, "kill"], [13.6, "confirm"], [14.6, "confirm"],
    [15.6, "idleB"], [16.9, "tray"], [17.2, "tray"], [17.9, "bilan"], [18.4, "bilan"],
    [19.4, "ok"], [21.4, "ok"], [22.4, "idleA"], [24.0, "cancel"], [25, "cancel"],
  ];
  function cursorAt(t) {
    const k = keys();
    let i = 0;
    while (i < k.length - 2 && t >= k[i + 1][0]) i++;
    const [t0, n0] = k[i], [t1, n1] = k[i + 1];
    const f = ease(clamp((t - t0) / (t1 - t0), 0, 1));
    const a = POS[n0], b = POS[n1];
    return { x: lerp(a.x, b.x, f), y: lerp(a.y, b.y, f) };
  }
  const cursorSvg = `<svg class="cursor" viewBox="0 0 22 28"><path d="M2 1.5v21l5.4-5 3.6 8.2 3.2-1.4-3.6-8H19z" fill="#fff" stroke="#000" stroke-width="1.6" stroke-linejoin="round"/></svg>`;

  RU.demo = {
    W: OUT_W, H: OUT_H, DUR,
    html(t) {
      if (!POS) measure();
      const c = cursorAt(t);
      const fade = Math.min(seg(t, 0, 0.4), 1 - seg(t, DUR - 0.5, DUR));
      return `<div class="desk theme-dark" style="width:${W}px;height:${H}px;opacity:${fade};transform:scale(${S});transform-origin:0 0">
        ${scene(t)}
        <div class="caption"><span>${caption(t)}</span></div>
        <div class="tag">Illustration</div>
        ${cursorSvg.replace('class="cursor"', `class="cursor" style="left:${c.x - 2}px;top:${c.y - 1}px"`)}</div>`;
    },
  };
})();
