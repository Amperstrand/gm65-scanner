#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackEvent {
    PowerUp,
    DecodeOk,
    TransmissionError,
    ConfigOk,
    ConfigError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedPulse {
    pub on_ms: u16,
    pub off_ms: u16,
}

const POWER_UP_PULSES: [LedPulse; 3] = [
    LedPulse {
        on_ms: 50,
        off_ms: 40,
    },
    LedPulse {
        on_ms: 100,
        off_ms: 40,
    },
    LedPulse {
        on_ms: 150,
        off_ms: 40,
    },
];
const DECODE_OK_PULSES: [LedPulse; 1] = [LedPulse {
    on_ms: 80,
    off_ms: 0,
}];
const TRANSMISSION_ERROR_PULSES: [LedPulse; 4] = [LedPulse {
    on_ms: 250,
    off_ms: 120,
}; 4];
const CONFIG_OK_PULSES: [LedPulse; 2] = [
    LedPulse {
        on_ms: 100,
        off_ms: 60,
    },
    LedPulse {
        on_ms: 60,
        off_ms: 60,
    },
];
const CONFIG_ERROR_PULSES: [LedPulse; 2] = [
    LedPulse {
        on_ms: 180,
        off_ms: 70,
    },
    LedPulse {
        on_ms: 80,
        off_ms: 70,
    },
];

pub const fn pulses_for(event: FeedbackEvent) -> &'static [LedPulse] {
    match event {
        FeedbackEvent::PowerUp => &POWER_UP_PULSES,
        FeedbackEvent::DecodeOk => &DECODE_OK_PULSES,
        FeedbackEvent::TransmissionError => &TRANSMISSION_ERROR_PULSES,
        FeedbackEvent::ConfigOk => &CONFIG_OK_PULSES,
        FeedbackEvent::ConfigError => &CONFIG_ERROR_PULSES,
    }
}

pub const fn profile_save_status(reboot: bool) -> &'static str {
    if reboot {
        "Settings saved; re-enumerating USB..."
    } else {
        "Settings saved"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_patterns_match_documented_lengths() {
        assert_eq!(pulses_for(FeedbackEvent::PowerUp).len(), 3);
        assert_eq!(pulses_for(FeedbackEvent::DecodeOk).len(), 1);
        assert_eq!(pulses_for(FeedbackEvent::TransmissionError).len(), 4);
        assert_eq!(pulses_for(FeedbackEvent::ConfigOk).len(), 2);
        assert_eq!(pulses_for(FeedbackEvent::ConfigError).len(), 2);
    }

    #[test]
    fn profile_save_status_distinguishes_reboot_requirement() {
        assert_eq!(profile_save_status(false), "Settings saved");
        assert_eq!(
            profile_save_status(true),
            "Settings saved; re-enumerating USB..."
        );
    }
}
