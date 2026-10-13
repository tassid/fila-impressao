const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const REFRESH_MS = 1500;
const STUCK_MS = 5000;

const el = (id) => document.getElementById(id);
const panel = el("panel");
const state = el("state");
const hero = el("hero");
const count = el("count");
const picker = el("printer");
const jobs = el("jobs");
const notice = el("notice");
const clearButton = el("clear");
const forceButton = el("force");

let lastJobs = "";
let lastPrinters = "";
let clearedAt = 0;
let forcing = false;

function tone(snapshot) {
  if (snapshot.connected === false) return "off";
  if (snapshot.problem || snapshot.jobs.some((job) => job.problem)) return "problem";
  if (snapshot.printing) return "busy";
  return snapshot.selected ? "ok" : "idle";
}

function headline(snapshot) {
  if (snapshot.connected === true) return `Conectada · ${snapshot.status}`;
  return snapshot.status;
}

function say(text, kind = "") {
  notice.textContent = text;
  notice.dataset.kind = kind;
}

function renderPrinters(snapshot) {
  const key = snapshot.printers.join("\n");
  if (key !== lastPrinters) {
    lastPrinters = key;
    picker.replaceChildren(...snapshot.printers.map((name) => new Option(name, name)));
  }
  picker.value = snapshot.selected;
}

function jobRow(job) {
  const row = document.createElement("li");
  row.className = "job";
  row.dataset.problem = job.problem;

  const edge = document.createElement("span");
  edge.className = "job-edge";

  const text = document.createElement("div");
  text.style.minWidth = "0";
  const name = document.createElement("div");
  name.className = "job-name";
  name.textContent = job.document || "Documento sem nome";
  const meta = document.createElement("div");
  meta.className = "job-meta";
  meta.textContent = job.pages > 0 ? `${job.status} · ${job.pages} pág` : job.status;
  text.append(name, meta);

  const cancel = document.createElement("button");
  cancel.className = "job-cancel";
  cancel.title = "Cancelar";
  cancel.setAttribute("aria-label", `Cancelar ${name.textContent}`);
  cancel.innerHTML = '<svg viewBox="0 0 12 12"><path d="M2 2l8 8M10 2l-8 8"/></svg>';
  cancel.addEventListener("click", () => run(() => invoke("cancel_job", { id: job.id })));

  row.append(edge, text, cancel);
  return row;
}

function renderJobs(snapshot) {
  const key = JSON.stringify(snapshot.jobs);
  if (key === lastJobs) return;
  lastJobs = key;
  jobs.replaceChildren(...snapshot.jobs.map(jobRow));
  el("empty").hidden = snapshot.jobs.length > 0;

  const total = String(snapshot.jobs.length);
  if (count.textContent !== total) {
    count.textContent = total;
    count.classList.remove("bump");
    void count.offsetWidth;
    count.classList.add("bump");
  }
  el("count-label").textContent = "na fila";
}

function render(snapshot) {
  const current = tone(snapshot);
  state.dataset.tone = current === "off" ? "idle" : current;
  hero.dataset.tone = current;
  hero.dataset.queued = snapshot.jobs.length > 0;
  el("state-text").textContent = headline(snapshot);

  renderPrinters(snapshot);
  renderJobs(snapshot);

  if (snapshot.jobs.length === 0) clearedAt = 0;
  const stuck = clearedAt > 0 && Date.now() - clearedAt >= STUCK_MS && snapshot.jobs.length > 0;
  forceButton.classList.toggle("urgent", stuck);
  if (stuck && !forcing) say("Tem job preso. Use Forçar.", "warn");

  clearButton.disabled = forcing || snapshot.jobs.length === 0;
  forceButton.disabled = forcing;
  forceButton.textContent = forcing ? "Forçando…" : "Forçar";
}

async function refresh() {
  try {
    render(await invoke("snapshot"));
  } catch (error) {
    say(String(error), "error");
  }
}

/** Runs a command, reports its failure in the notice line, then refreshes. */
async function run(action) {
  say("");
  try {
    await action();
  } catch (error) {
    say(String(error), "error");
  }
  await refresh();
}

picker.addEventListener("change", () => {
  clearedAt = 0;
  lastJobs = "";
  run(() => invoke("select_printer", { name: picker.value }));
});

clearButton.addEventListener("click", () =>
  run(async () => {
    clearedAt = Date.now();
    const refused = await invoke("clear_all");
    if (refused > 0) say(`${refused} job(s) recusaram cancelar. Use Forçar.`, "warn");
  }),
);

forceButton.addEventListener("click", () => (el("confirm").hidden = false));
el("confirm-no").addEventListener("click", () => (el("confirm").hidden = true));
el("confirm-yes").addEventListener("click", async () => {
  el("confirm").hidden = true;
  forcing = true;
  say("Aguardando permissão de administrador…");
  await refresh();
  try {
    say(await invoke("force_clear"));
    clearedAt = 0;
  } catch (error) {
    say(String(error), "error");
  }
  forcing = false;
  await refresh();
});

el("settings-toggle").addEventListener("click", async () => {
  el("autostart").checked = await invoke("get_autostart");
  el("settings").hidden = false;
});
el("settings-close").addEventListener("click", () => (el("settings").hidden = true));
el("quit").addEventListener("click", () => invoke("quit"));
el("autostart").addEventListener("change", async (event) => {
  try {
    await invoke("set_autostart", { enabled: event.target.checked });
  } catch (error) {
    event.target.checked = !event.target.checked;
    say(String(error), "error");
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  const open = [el("confirm"), el("settings")].find((sheet) => !sheet.hidden);
  if (open) open.hidden = true;
  else invoke("hide_panel");
});
document.addEventListener("contextmenu", (event) => event.preventDefault());

function playEntrance() {
  panel.classList.remove("entering");
  void panel.offsetWidth;
  panel.classList.add("entering");
}

listen("panel-shown", () => {
  playEntrance();
  refresh();
});

playEntrance();
refresh();
setInterval(refresh, REFRESH_MS);
