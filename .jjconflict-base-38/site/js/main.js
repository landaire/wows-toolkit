import { ARMOR_SCALE, armorColor, OUTCOME_LABEL } from "./armor.js";

const root = document.documentElement;
const body = document.body;
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];

function setPressed(attr, value) {
    for (const b of $$(`[${attr}]`)) b.setAttribute("aria-pressed", String(b.getAttribute(attr) === value));
}

let scene = null;
const prefs = {
    theme: root.dataset.theme,
    mode: "sea",
};

function applyTheme(theme, persist) {
    prefs.theme = theme;
    root.dataset.theme = theme;
    if (persist) localStorage.setItem("wt-theme", theme);
    scene?.setTheme(theme);
    for (const b of $$("[data-theme-toggle]")) {
        b.setAttribute("aria-label", theme === "dark" ? "Switch to light theme" : "Switch to dark theme");
    }
}

function liveSrc(fig) {
    return `${fig.dataset.live}?theme=${prefs.theme}`;
}

// Live demos load only when started: the renderer demo is a few megabytes.
for (const fig of $$(".shot.live")) {
    fig.querySelector(".live-play").addEventListener("click", () => {
        const frame = document.createElement("iframe");
        frame.title = "Replay renderer running in the browser";
        frame.src = liveSrc(fig);
        fig.querySelector(".live-frame").replaceChildren(frame);
    });
}

let pendingMode = null;

function applyMode(mode) {
    prefs.mode = mode;
    for (const b of $$("[data-mode-toggle]")) b.setAttribute("aria-pressed", String(mode === "armor"));
    body.classList.toggle("armor-mode", mode === "armor");
    scene?.setMode(mode);
    if (mode === "sea") hideTooltip();
}

for (const b of $$("[data-theme-toggle]")) {
    b.addEventListener("click", () => applyTheme(prefs.theme === "dark" ? "light" : "dark", true));
}
for (const b of $$("[data-mode-toggle]")) {
    b.addEventListener("click", () => applyMode(prefs.mode === "armor" ? "sea" : "armor"));
}
for (const b of $$("[data-mode-jump]")) {
    b.addEventListener("click", () => {
        if (window.scrollY < window.innerHeight * 0.1) {
            applyMode(b.dataset.modeJump);
            return;
        }
        // Switch once the page is back at the model.
        pendingMode = b.dataset.modeJump;
        window.scrollTo({ top: 0, behavior: "smooth" });
    });
}

// A change to the system scheme wins over an earlier manual choice.
matchMedia("(prefers-color-scheme: light)").addEventListener("change", (e) => {
    localStorage.removeItem("wt-theme");
    applyTheme(e.matches ? "light" : "dark", false);
});

applyTheme(prefs.theme, false);

// Armor HUD.
const legend = $("#legend");
for (const [max, rgb] of ARMOR_SCALE) {
    const cell = document.createElement("span");
    cell.style.background = `rgb(${rgb.join(",")})`;
    cell.title = `up to ${max} mm`;
    legend.appendChild(cell);
}

for (const b of $$("[data-shell]")) {
    b.addEventListener("click", () => {
        setPressed("data-shell", b.dataset.shell);
        scene?.setShell(b.dataset.shell);
    });
}

const rangeIn = $("#range-km");
const rangeOut = $("#range-out");
rangeIn.addEventListener("input", () => {
    rangeOut.textContent = `${rangeIn.value} km`;
    scene?.setRange(Number(rangeIn.value));
});

const sectionIn = $("#section");
const sectionOut = $("#section-out");
sectionIn.addEventListener("input", () => {
    const v = Number(sectionIn.value);
    sectionOut.textContent = v >= 0.999 ? "off" : `${Math.round((1 - v) * 200)}%`;
    scene?.setSection(v);
});

const tooltip = $("#tooltip");
function hideTooltip() {
    tooltip.classList.remove("shown");
}

function showTooltip(hover) {
    if (!hover) return hideTooltip();
    const { zone, angle, x, y } = hover;
    const rgb = armorColor(zone.mm).join(",");
    const eff = zone.mm / Math.cos((Math.min(angle, 89) * Math.PI) / 180);
    tooltip.innerHTML = `
        <div class="zone">${zone.zone}</div>
        <div class="plate"><span class="swatch" style="width:10px;height:10px;display:inline-block;background:rgb(${rgb})"></span>${zone.plate}</div>
        <dl>
            <dt>Thickness</dt><dd>${zone.mm} mm</dd>
            <dt>View angle</dt><dd>${angle.toFixed(1)}&deg;</dd>
            <dt>Line of sight</dt><dd>${eff >= 2000 ? "&gt;2000" : eff.toFixed(0)} mm</dd>
        </dl>`;
    const pad = 16;
    const w = tooltip.offsetWidth || 200;
    const h = tooltip.offsetHeight || 100;
    const left = Math.min(x, window.innerWidth - w - pad * 2);
    const top = Math.min(y, window.innerHeight - h - pad * 2);
    tooltip.style.left = `${left}px`;
    tooltip.style.top = `${top}px`;
    tooltip.classList.add("shown");
}

const OUTCOME_CLASS = {
    CITADEL: "good",
    PEN: "good",
    HE_PEN: "good",
    OVERPEN: "mid",
    RICOCHET: "bad",
    NO_PEN: "bad",
    SHATTER: "bad",
    MISS: "mid",
};

function showShot(shot) {
    $("#shot-card").hidden = false;
    const outcome = $("#shot-outcome");
    outcome.textContent = OUTCOME_LABEL[shot.outcome];
    outcome.className = `outcome ${OUTCOME_CLASS[shot.outcome]}`;
    const rows = shot.plates.slice(0, 7).map((p) => {
        const rgb = armorColor(p.mm).join(",");
        const result = p.note && p.result === "PEN" ? `${p.result}*` : p.result;
        return `<tr title="${p.note ?? ""}">
            <td><span class="swatch" style="background:rgb(${rgb})"></span>${p.plate}</td>
            <td class="num">${p.mm}</td>
            <td class="num">${p.angle.toFixed(0)}&deg;</td>
            <td class="num">${p.effective ? p.effective.toFixed(0) : "-"}</td>
            <td class="num">${result}</td>
        </tr>`;
    });
    $("#shot-plates").innerHTML = rows.join("") || `<tr><td colspan="5">No plates on this line.</td></tr>`;
}

// The model controls float above the model in the sea view and dock at the
// bottom of the screen in the armor view, where the model fills the frame.
const modelCtl = $("#model-ctl");
const ctlPos = { x: null, y: null };
function placeModelCtl(x, y) {
    if (window.innerWidth < 820) {
        modelCtl.style.transform = "";
        ctlPos.x = null;
        return;
    }
    const w = modelCtl.offsetWidth;
    const h = modelCtl.offsetHeight;
    let tx;
    let ty;
    if (prefs.mode === "armor") {
        tx = window.innerWidth * 0.58 - w / 2;
        ty = window.innerHeight - h - 8;
    } else {
        tx = Math.min(Math.max(x - w / 2, 24), window.innerWidth - w - 24);
        ty = Math.max(y - h - 12, 76);
    }
    if (ctlPos.x === null) {
        ctlPos.x = tx;
        ctlPos.y = ty;
    }
    ctlPos.x += (tx - ctlPos.x) * 0.12;
    ctlPos.y += (ty - ctlPos.y) * 0.12;
    modelCtl.style.transform = `translate(${ctlPos.x.toFixed(1)}px, ${ctlPos.y.toFixed(1)}px)`;
}

// Interaction with the scene only where the page is not in the way.
function canInteract(e) {
    const t = e.target;
    if (!(t instanceof Element)) return true;
    if (t === document.getElementById("scene")) return true;
    if (t.closest("a, button, input, label, .card, .model-ctl, .shot, .feature-copy, .section-head, header, footer, code"))
        return false;
    return t.closest(".hero") !== null || t === body || t === root || t.tagName === "MAIN";
}

// Ruler labels and axis gizmo drawn over the armor view.
const ruler = $("#ruler");
const gizmo = $("#gizmo");
function placeOverlay({ armor, ruler: marks, axes }) {
    if (armor <= 0.01) return;
    while (ruler.children.length < marks.length) ruler.appendChild(document.createElement("span"));
    [...ruler.children].forEach((chip, i) => {
        const mark = marks[i];
        chip.hidden = !mark || mark.behind;
        if (chip.hidden) return;
        chip.textContent = mark.label;
        chip.style.transform = `translate(${mark.at[0].toFixed(1)}px, ${mark.at[1].toFixed(1)}px) translate(-50%, 6px)`;
    });
    for (const [axis, [dx, dy]] of Object.entries(axes)) {
        const x = (dx * 20).toFixed(2);
        const y = (dy * 20).toFixed(2);
        for (const el of gizmo.querySelectorAll(`[data-axis="${axis}"]`)) {
            if (el.tagName === "line") {
                el.setAttribute("x2", x);
                el.setAttribute("y2", y);
            } else if (el.tagName === "circle") {
                el.setAttribute("cx", x);
                el.setAttribute("cy", y);
            } else {
                el.setAttribute("x", x);
                el.setAttribute("y", y);
            }
        }
    }
}

async function startScene() {
    const canvas = $("#scene");
    if (!document.createElement("canvas").getContext("webgl2")) {
        body.classList.add("no-webgl");
        return;
    }
    try {
        const { createScene } = await import("./scene.js");
        scene = await createScene(canvas, {
            theme: prefs.theme,
            canInteract,
            onHover: (h) => (prefs.mode === "armor" ? showTooltip(h) : hideTooltip()),
            onShot: showShot,
            onAnchor: placeModelCtl,
            onOverlay: placeOverlay,
        });
        scene.setMode(prefs.mode);
        scene.setRange(Number(rangeIn.value));
    } catch (err) {
        console.error(err);
        body.classList.add("no-webgl");
    }
}
startScene();

// Scroll state: dim the scene and hide the model controls past the hero.
function onScroll() {
    const y = window.scrollY;
    const h = window.innerHeight;
    const scrolled = y > h * 0.35;
    body.classList.toggle("scrolled", scrolled);
    body.classList.toggle("controls-hidden", y > h * 0.6);
    scene?.setDimmed(y > h * 1.2);
    if (pendingMode && y < h * 0.05) {
        applyMode(pendingMode);
        pendingMode = null;
    } else if (!pendingMode && scrolled && prefs.mode === "armor") {
        applyMode("sea");
    }
}
window.addEventListener("scroll", onScroll, { passive: true });
onScroll();

// Reveal on scroll.
const io = new IntersectionObserver(
    (entries) => {
        for (const e of entries) {
            if (e.isIntersecting) {
                e.target.classList.add("in");
                io.unobserve(e.target);
            }
        }
    },
    { rootMargin: "0px 0px -10% 0px" },
);
for (const el of $$(".reveal")) io.observe(el);

// Figures with more than one view switch between them without opening the lightbox.
for (const b of $$(".shot.views [data-show]")) {
    b.addEventListener("click", (e) => {
        e.stopPropagation();
        const fig = b.closest(".shot");
        fig.dataset.view = b.dataset.show;
        for (const other of fig.querySelectorAll("[data-show]")) {
            other.setAttribute("aria-pressed", String(other === b));
        }
    });
}

// Lightbox. Screenshot figures act as buttons that open it.
const lightbox = $("#lightbox");
let lightboxOpener = null;
function openLightbox(fig) {
    const img = [...fig.querySelectorAll("img")].find((i) => i.offsetParent !== null);
    if (!img) return;
    lightbox.querySelector("img").src = img.currentSrc || img.src;
    lightbox.querySelector("img").alt = img.alt;
    lightbox.classList.add("open");
    lightboxOpener = fig;
    lightbox.focus();
}
function closeLightbox() {
    if (!lightbox.classList.contains("open")) return;
    lightbox.classList.remove("open");
    lightboxOpener?.focus();
    lightboxOpener = null;
}
for (const fig of $$("[data-zoom]")) {
    fig.tabIndex = 0;
    fig.setAttribute("role", "button");
    fig.setAttribute("aria-label", `Enlarge screenshot: ${fig.querySelector(".bar span")?.textContent ?? ""}`);
    fig.addEventListener("click", () => openLightbox(fig));
    fig.addEventListener("keydown", (e) => {
        if (e.target !== fig || (e.key !== "Enter" && e.key !== " ")) return;
        e.preventDefault();
        openLightbox(fig);
    });
}
lightbox.addEventListener("click", closeLightbox);
document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeLightbox();
});

// Copy the nix command.
const cmd = $(".cmd");
cmd.addEventListener("click", async () => {
    try {
        await navigator.clipboard.writeText(cmd.textContent.trim());
        cmd.dataset.copied = "";
        setTimeout(() => delete cmd.dataset.copied, 1200);
    } catch {
        // Clipboard access can be refused; the text stays selectable.
    }
});

// Latest release: version tag and per-platform assets.
(async () => {
    try {
        const resp = await fetch("https://api.github.com/repos/landaire/wows-toolkit/releases/latest");
        if (!resp.ok) return;
        const data = await resp.json();
        if (data.tag_name) $("#download-version").textContent = data.tag_name;
        const find = (pred) => data.assets?.find((a) => pred(a.name));
        const assets = {
            windows: find((n) => n.endsWith(".msi")) || find((n) => n.endsWith("_windows.zip")),
            macos: find((n) => n.endsWith("_macos.dmg")),
            linux: find((n) => n.endsWith(".flatpak")),
        };
        for (const a of $$("[data-asset]")) {
            const asset = assets[a.dataset.asset];
            if (asset) a.href = asset.browser_download_url;
        }
        const ua = navigator.userAgent.toLowerCase();
        const mine = ua.includes("mac") ? assets.macos : ua.includes("linux") ? assets.linux : assets.windows;
        if (mine) for (const a of $$("#download-btn, [data-download]")) a.href = mine.browser_download_url;
    } catch {
        // Offline or rate limited: the links fall back to the releases page.
    }
})();

// The Discord invite is ROT13-encoded so scrapers cannot harvest it from the
// page source. It is decoded only on click.
function rot13(s) {
    return s.replace(/[a-z]/gi, (c) => {
        const base = c <= "Z" ? 65 : 97;
        return String.fromCharCode(((c.charCodeAt(0) - base + 13) % 26) + base);
    });
}
for (const a of $$("a.js-discord")) {
    a.addEventListener("click", (e) => {
        e.preventDefault();
        window.open(rot13(a.dataset.h), "_blank", "noopener,noreferrer");
    });
}
