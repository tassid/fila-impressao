//! Translates spooler status bit flags into text for the window.

const JOB_PAUSED: u32 = 0x1;
const JOB_ERROR: u32 = 0x2;
const JOB_DELETING: u32 = 0x4;
const JOB_SPOOLING: u32 = 0x8;
const JOB_PRINTING: u32 = 0x10;
const JOB_OFFLINE: u32 = 0x20;
const JOB_PAPEROUT: u32 = 0x40;
const JOB_PRINTED: u32 = 0x80;
const JOB_BLOCKED: u32 = 0x200;
const JOB_USER_INTERVENTION: u32 = 0x400;
const JOB_COMPLETE: u32 = 0x1000;

const PRINTER_PAUSED: u32 = 0x1;
const PRINTER_ERROR: u32 = 0x2;
const PRINTER_PAPER_JAM: u32 = 0x8;
const PRINTER_PAPER_OUT: u32 = 0x10;
const PRINTER_OFFLINE: u32 = 0x80;
const PRINTER_PRINTING: u32 = 0x400;
const PRINTER_NO_TONER: u32 = 0x40000;
const PRINTER_USER_INTERVENTION: u32 = 0x100000;
const PRINTER_DOOR_OPEN: u32 = 0x400000;

const ATTRIBUTE_WORK_OFFLINE: u32 = 0x400;

/// `custom` is the driver's own status string; when present it wins.
pub fn job_status_text(flags: u32, custom: &str) -> String {
    if !custom.trim().is_empty() {
        return custom.trim().to_string();
    }
    let text = if flags & JOB_DELETING != 0 {
        "Excluindo"
    } else if flags & JOB_ERROR != 0 {
        "Erro"
    } else if flags & JOB_OFFLINE != 0 {
        "Impressora offline"
    } else if flags & JOB_PAPEROUT != 0 {
        "Sem papel"
    } else if flags & JOB_USER_INTERVENTION != 0 {
        "Precisa de atenção"
    } else if flags & JOB_BLOCKED != 0 {
        "Bloqueado"
    } else if flags & JOB_PAUSED != 0 {
        "Pausado"
    } else if flags & JOB_PRINTING != 0 {
        "Imprimindo"
    } else if flags & JOB_SPOOLING != 0 {
        "Enviando"
    } else if flags & (JOB_PRINTED | JOB_COMPLETE) != 0 {
        "Impresso"
    } else {
        "Na fila"
    };
    text.to_string()
}

pub fn printer_status_text(status: u32, attributes: u32) -> &'static str {
    if status & PRINTER_OFFLINE != 0 || attributes & ATTRIBUTE_WORK_OFFLINE != 0 {
        "Offline (desligada ou desconectada)"
    } else if status & PRINTER_PAPER_JAM != 0 {
        "Papel atolado"
    } else if status & PRINTER_PAPER_OUT != 0 {
        "Sem papel"
    } else if status & PRINTER_DOOR_OPEN != 0 {
        "Tampa aberta"
    } else if status & PRINTER_NO_TONER != 0 {
        "Sem tinta"
    } else if status & PRINTER_USER_INTERVENTION != 0 {
        "Precisa de atenção"
    } else if status & PRINTER_ERROR != 0 {
        "Erro"
    } else if status & PRINTER_PAUSED != 0 {
        "Pausada"
    } else if status & PRINTER_PRINTING != 0 {
        "Imprimindo"
    } else {
        "Pronta"
    }
}

/// True when the job is stuck on something the user has to deal with.
pub fn job_has_problem(flags: u32) -> bool {
    flags & (JOB_ERROR | JOB_OFFLINE | JOB_PAPEROUT | JOB_USER_INTERVENTION | JOB_BLOCKED) != 0
}

pub fn printer_has_problem(status: u32, attributes: u32) -> bool {
    let problems = PRINTER_OFFLINE
        | PRINTER_PAPER_JAM
        | PRINTER_PAPER_OUT
        | PRINTER_DOOR_OPEN
        | PRINTER_NO_TONER
        | PRINTER_USER_INTERVENTION
        | PRINTER_ERROR;
    status & problems != 0 || attributes & ATTRIBUTE_WORK_OFFLINE != 0
}

pub fn printer_is_printing(status: u32) -> bool {
    status & PRINTER_PRINTING != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_are_flagged_but_normal_states_are_not() {
        assert!(job_has_problem(JOB_ERROR | JOB_PRINTING));
        assert!(job_has_problem(JOB_PAPEROUT));
        assert!(!job_has_problem(JOB_PRINTING | JOB_SPOOLING | JOB_PAUSED | JOB_DELETING));
        assert!(printer_has_problem(PRINTER_PAPER_JAM, 0));
        assert!(printer_has_problem(0, ATTRIBUTE_WORK_OFFLINE));
        assert!(!printer_has_problem(PRINTER_PRINTING | PRINTER_PAUSED, 0));
        assert!(printer_is_printing(PRINTER_PRINTING));
        assert!(!printer_is_printing(0));
    }

    #[test]
    fn job_without_flags_is_waiting() {
        assert_eq!(job_status_text(0, ""), "Na fila");
    }

    #[test]
    fn job_driver_text_wins_over_flags() {
        assert_eq!(job_status_text(JOB_PRINTING, " Aquecendo "), "Aquecendo");
    }

    #[test]
    fn job_deleting_wins_over_printing() {
        assert_eq!(job_status_text(JOB_DELETING | JOB_PRINTING, ""), "Excluindo");
    }

    #[test]
    fn job_error_wins_over_printing() {
        assert_eq!(job_status_text(JOB_ERROR | JOB_PRINTING, ""), "Erro");
    }

    #[test]
    fn job_paused_and_spooling() {
        assert_eq!(job_status_text(JOB_PAUSED, ""), "Pausado");
        assert_eq!(job_status_text(JOB_SPOOLING, ""), "Enviando");
    }

    #[test]
    fn printer_ready_when_no_flags() {
        assert_eq!(printer_status_text(0, 0), "Pronta");
    }

    #[test]
    fn printer_offline_from_status_or_attribute() {
        let offline = "Offline (desligada ou desconectada)";
        assert_eq!(printer_status_text(PRINTER_OFFLINE, 0), offline);
        assert_eq!(printer_status_text(0, ATTRIBUTE_WORK_OFFLINE), offline);
    }

    #[test]
    fn printer_problem_wins_over_printing() {
        assert_eq!(printer_status_text(PRINTER_PAPER_OUT | PRINTER_PRINTING, 0), "Sem papel");
        assert_eq!(printer_status_text(PRINTER_PAPER_JAM | PRINTER_PAPER_OUT, 0), "Papel atolado");
    }
}
