/// Default Run/S&P digital function-key messages.
///
/// This intentionally starts as a copy of the CW message template so digital
/// messages can evolve independently.
pub const DEFAULT_DIGITAL_MESSAGES: &str = r#"###################
#   RUN Messages
###################
F1 Cq,Cq Cq Test {STATION_CALLSIGN} {STATION_CALLSIGN}
F2 Exch,{EXCH}
F3 Tu,Tu
F4 {STATION_CALLSIGN},{STATION_CALLSIGN}
F5 His Call,{CALL}
F6 Repeat,{EXCH} {EXCH}
F7 -,
F8 Agn?,Agn Agn
F9 Nr?,Nr Agn
F10 Call?,Call Agn
F11 -,
F12 Clear,{Action:Clear}
#
###################
#   S&P Messages
###################
F1 Qrl?,Qrl? de {STATION_CALLSIGN}
F2 Exch,{EXCH}
F3 Tu,Tu
F4 {STATION_CALLSIGN},{STATION_CALLSIGN}
F5 His Call,{CALL}
F6 Repeat,{EXCH} {EXCH}
F7 ?,?
F8 Agn?,Agn?
F9 Nr?,Nr?
F10 Call?,Cl?
F11 -,
F12 Clear,{Action:Clear}
"#;
