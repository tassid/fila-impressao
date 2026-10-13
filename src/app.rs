//! The window. All spooler calls run on a worker thread so the UI never freezes.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::spooler::{self, Job, PrinterState};
use crate::{force, status};

const REFRESH_EVERY: Duration = Duration::from_secs(2);
const STUCK_AFTER: Duration = Duration::from_secs(5);

enum Command {
    Select(String),
    Cancel(u32),
    ClearAll,
    Force,
}

#[derive(Default)]
struct Snapshot {
    printers: Vec<String>,
    selected: String,
    state: Option<Result<PrinterState, String>>,
    jobs: Vec<Job>,
    /// Result of the command that triggered this snapshot, if it has one to report.
    notice: Option<Result<String, String>>,
    force_finished: bool,
}

fn pick_printer(printers: &[String]) -> String {
    let hp = printers.iter().find(|p| p.to_lowercase().contains("hp")).cloned();
    let default = spooler::default_printer().filter(|d| printers.contains(d));
    hp.or(default).or_else(|| printers.first().cloned()).unwrap_or_default()
}

fn worker(commands: Receiver<Command>, snapshots: Sender<Snapshot>, ctx: egui::Context) {
    let mut selected = String::new();
    let mut pending = None;
    loop {
        let mut snapshot = Snapshot::default();
        match pending.take() {
            Some(Command::Select(name)) => selected = name,
            Some(Command::Cancel(id)) => {
                if let Err(e) = spooler::cancel_job(&selected, id) {
                    snapshot.notice = Some(Err(format!("Não consegui cancelar: {e}")));
                }
            }
            Some(Command::ClearAll) => match spooler::clear_all(&selected) {
                Ok(0) => {}
                Ok(refused) => {
                    snapshot.notice = Some(Err(format!("{refused} job(s) recusaram cancelar. Use Forçar.")));
                }
                Err(e) => snapshot.notice = Some(Err(format!("Não consegui limpar: {e}"))),
            },
            Some(Command::Force) => {
                snapshot.notice = Some(force::run_elevated());
                snapshot.force_finished = true;
            }
            None => {}
        }

        match spooler::list_printers() {
            Ok(printers) => {
                if !printers.contains(&selected) {
                    selected = pick_printer(&printers);
                }
                snapshot.printers = printers;
            }
            Err(e) => snapshot.state = Some(Err(format!("Spooler não responde: {e}"))),
        }
        snapshot.selected = selected.clone();
        if snapshot.state.is_none() && !selected.is_empty() {
            snapshot.state = Some(spooler::printer_state(&selected).map_err(|e| e.to_string()));
            snapshot.jobs = spooler::list_jobs(&selected).unwrap_or_default();
        }

        if snapshots.send(snapshot).is_err() {
            return;
        }
        ctx.request_repaint();

        match commands.recv_timeout(REFRESH_EVERY) {
            Ok(command) => pending = Some(command),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

pub struct App {
    commands: Sender<Command>,
    snapshots: Receiver<Snapshot>,
    snapshot: Snapshot,
    notice: Option<Result<String, String>>,
    cleared_at: Option<Instant>,
    confirming_force: bool,
    forcing: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (snapshot_tx, snapshots) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || worker(command_rx, snapshot_tx, ctx));
        Self {
            commands,
            snapshots,
            snapshot: Snapshot::default(),
            notice: None,
            cleared_at: None,
            confirming_force: false,
            forcing: false,
        }
    }

    fn send(&mut self, command: Command) {
        self.notice = None;
        let _ = self.commands.send(command);
    }

    fn receive(&mut self) {
        while let Ok(mut snapshot) = self.snapshots.try_recv() {
            if let Some(notice) = snapshot.notice.take() {
                self.notice = Some(notice);
            }
            if snapshot.force_finished {
                self.forcing = false;
                self.cleared_at = None;
            }
            self.snapshot = snapshot;
        }
        if self.snapshot.jobs.is_empty() {
            self.cleared_at = None;
        }
    }

    fn stuck(&self) -> bool {
        self.cleared_at.is_some_and(|at| at.elapsed() >= STUCK_AFTER) && !self.snapshot.jobs.is_empty()
    }

    fn status_line(&self) -> String {
        match &self.snapshot.state {
            None => "Carregando…".to_string(),
            Some(Err(e)) => format!("Erro: {e}"),
            Some(Ok(state)) => {
                let text = status::printer_status_text(state.status, state.attributes);
                match self.snapshot.jobs.len() {
                    0 => format!("{text} · fila vazia"),
                    n => format!("{text} · {n} na fila"),
                }
            }
        }
    }

    fn actions(&mut self, ui: &mut egui::Ui) {
        let stuck = self.stuck();
        ui.add_space(6.0);
        if stuck {
            ui.colored_label(ui.visuals().warn_fg_color, "Tem job preso. Use Forçar.");
        }
        match &self.notice {
            Some(Ok(text)) => {
                ui.label(text);
            }
            Some(Err(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            None => {}
        }
        ui.horizontal(|ui| {
            let has_jobs = !self.snapshot.jobs.is_empty();
            if ui.add_enabled(has_jobs && !self.forcing, egui::Button::new("Limpar tudo")).clicked() {
                self.cleared_at = Some(Instant::now());
                self.send(Command::ClearAll);
            }
            let mut force = egui::Button::new(if self.forcing { "Forçando…" } else { "Forçar" });
            if stuck {
                force = force.fill(ui.visuals().selection.bg_fill);
            }
            if ui
                .add_enabled(!self.forcing, force)
                .on_hover_text("Reinicia o spooler e apaga a fila de todas as impressoras. Pede administrador.")
                .clicked()
            {
                self.confirming_force = true;
            }
        });
        ui.add_space(6.0);
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        if self.snapshot.jobs.is_empty() {
            ui.weak("Nada na fila.");
            return;
        }
        let mut cancel = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("jobs").num_columns(4).striped(true).show(ui, |ui| {
                for job in &self.snapshot.jobs {
                    if ui.small_button("Cancelar").clicked() {
                        cancel = Some(job.id);
                    }
                    ui.label(status::job_status_text(job.status, &job.status_text));
                    if job.total_pages > 0 {
                        ui.label(format!("{} pág", job.total_pages));
                    } else {
                        ui.label("");
                    }
                    ui.add(egui::Label::new(&job.document).truncate());
                    ui.end_row();
                }
            });
        });
        if let Some(id) = cancel {
            self.send(Command::Cancel(id));
        }
    }

    fn force_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("Forçar limpeza?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Isso reinicia o spooler do Windows e apaga a fila de TODAS as impressoras, não só desta.");
                ui.label("O Windows vai pedir permissão de administrador.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Forçar").clicked() {
                        self.confirming_force = false;
                        self.forcing = true;
                        self.send(Command::Force);
                    }
                    if ui.button("Voltar").clicked() {
                        self.confirming_force = false;
                    }
                });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| self.actions(ui));

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.label("Impressora:");
                egui::ComboBox::from_id_salt("printer")
                    .selected_text(self.snapshot.selected.as_str())
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for printer in &self.snapshot.printers {
                            let current = *printer == self.snapshot.selected;
                            if ui.selectable_label(current, printer).clicked() && !current {
                                picked = Some(printer.clone());
                            }
                        }
                    });
            });
            if let Some(printer) = picked {
                self.cleared_at = None;
                self.send(Command::Select(printer));
            }
            ui.label(self.status_line());
            ui.separator();
            self.job_list(ui);
        });

        if self.confirming_force {
            self.force_dialog(ctx);
        }
        // Keeps the "job preso" hint appearing on time even with no input.
        if self.cleared_at.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}
