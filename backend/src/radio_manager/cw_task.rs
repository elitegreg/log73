use super::keyers::{CwSerialDevice, cw_keyer_for_config};
use super::voice::{VoiceDataPttGuard, spawn_voice_playback_thread};
use crate::cw;
use crate::db::RadioConfig;
use crate::radio::mode_is_phone;
use crate::voice_keyer::{VoiceKeyer, VoicePlaybackThread};
use crate::voice_messages;
use futures_util::future::BoxFuture;
use radio_cat_rs::{ChangeFlags, Radio, StateField, StateUpdate};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, warn};

pub(super) enum CwTaskCommand {
    SendMessage {
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
        completed: oneshot::Sender<Result<(), String>>,
    },
    SendText {
        text: String,
        wait_for_completion: bool,
        completed: oneshot::Sender<Result<(), String>>,
    },
    Stop,
    SetWpm(u8),
    Shutdown,
}

enum PendingCwPayload {
    Message {
        mode: String,
        keys: Vec<String>,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    Text {
        text: String,
        wait_for_completion: bool,
    },
}

struct PendingCwSend {
    payload: PendingCwPayload,
    completed: oneshot::Sender<Result<(), String>>,
}

pub(super) async fn run_cw_task(
    config: RadioConfig,
    radio: Radio,
    shared_cw_serial_keyer: Option<CwSerialDevice>,
    voice_keyer: VoiceKeyer,
    mut commands: mpsc::Receiver<CwTaskCommand>,
) {
    let mut controller =
        CwController::new(&config, radio, shared_cw_serial_keyer, voice_keyer).await;
    let mut pending = VecDeque::new();

    loop {
        let (next_send, prepend_space) = if let Some(send) = pending.pop_front() {
            (Some(send), true)
        } else {
            match commands.recv().await {
                Some(CwTaskCommand::SendMessage {
                    mode,
                    keys,
                    fields,
                    completed,
                }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Message { mode, keys, fields },
                        completed,
                    }),
                    false,
                ),
                Some(CwTaskCommand::SendText {
                    text,
                    wait_for_completion,
                    completed,
                }) => (
                    Some(PendingCwSend {
                        payload: PendingCwPayload::Text {
                            text,
                            wait_for_completion,
                        },
                        completed,
                    }),
                    false,
                ),
                Some(CwTaskCommand::Stop) => {
                    debug!(radio_id = config.id, "cw task received stop command");
                    controller.stop().await;
                    continue;
                }
                Some(CwTaskCommand::SetWpm(wpm)) => {
                    debug!(
                        radio_id = config.id,
                        wpm, "cw task received set_wpm command"
                    );
                    controller.set_wpm(wpm).await;
                    continue;
                }
                Some(CwTaskCommand::Shutdown) | None => {
                    debug!(radio_id = config.id, "cw task received shutdown command");
                    fail_pending_cw_sends(&mut pending, "cw shutdown");
                    break;
                }
            }
        };

        let Some(send) = next_send else {
            continue;
        };
        debug!(
            radio_id = config.id,
            pending_count = pending.len(),
            "cw task starting queued send"
        );
        let result = match &send.payload {
            PendingCwPayload::Message { mode, keys, fields } => {
                controller
                    .send_message(
                        mode,
                        keys,
                        fields,
                        prepend_space,
                        &mut commands,
                        &mut pending,
                    )
                    .await
            }
            PendingCwPayload::Text {
                text,
                wait_for_completion,
            } => {
                controller
                    .send_text(text, *wait_for_completion, &mut commands, &mut pending)
                    .await
            }
        };
        debug!(
            radio_id = config.id,
            ?result,
            remaining_pending = pending.len(),
            "cw task send command finished"
        );
        let should_shutdown = matches!(
            result.as_ref().err().map(String::as_str),
            Some("cw shutdown" | "keying shutdown")
        );
        let _ = send.completed.send(result);
        if should_shutdown {
            break;
        }
    }

    controller.close().await;
}

struct CwController {
    radio_id: i64,
    radio: Radio,
    messages: String,
    voice_messages: String,
    voice_playback: Option<VoicePlaybackThread>,
    voice_data_ptt_supported: bool,
    keyer: Option<Box<dyn CwKeyer>>,
}

impl CwController {
    async fn new(
        config: &RadioConfig,
        radio: Radio,
        shared_cw_serial_keyer: Option<CwSerialDevice>,
        voice_keyer: VoiceKeyer,
    ) -> Self {
        if let Err(error) = voice_keyer.sync_radio_messages(config) {
            warn!(radio_id = config.id, %error, "failed to sync voice keyer registrations for radio");
        }
        let voice_data_ptt_supported = radio
            .capabilities()
            .tx
            .map(|tx| tx.ptt.can_write())
            .unwrap_or(false);
        let mut controller = Self {
            radio_id: config.id,
            radio: radio.clone(),
            messages: config.cw_messages.clone(),
            voice_messages: config.voice_messages.clone(),
            voice_playback: spawn_voice_playback_thread(config, voice_keyer),
            voice_data_ptt_supported,
            keyer: cw_keyer_for_config(config, radio, shared_cw_serial_keyer).await,
        };
        controller.connect().await;
        controller
    }

    async fn connect(&mut self) {
        if let Some(keyer) = self.keyer.as_mut() {
            keyer.connect(self.radio_id).await;
        } else {
            debug!(radio_id = self.radio_id, "CW keying disabled for radio");
        }
    }

    async fn send_message(
        &mut self,
        mode: &str,
        keys: &[String],
        fields: &serde_json::Map<String, serde_json::Value>,
        prepend_space: bool,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let Some(logger_mode) = self.radio_logger_mode() else {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                "ignoring message send; unable to determine radio mode"
            );
            return Ok(());
        };
        if mode_is_phone(&logger_mode) {
            return self
                .send_voice_messages(mode, keys, fields, commands, pending)
                .await;
        }
        if logger_mode != "CW" && logger_mode != "CW-R" {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                radio_mode = %logger_mode,
                "ignoring message send; radio mode is not CW or phone"
            );
            return Ok(());
        }

        let mut rendered_parts = Vec::new();
        for key in keys {
            let Some(rendered_text) = cw::render(&self.messages, mode, key, fields) else {
                warn!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    key,
                    "unknown cw message"
                );
                return Err("unknown cw message".to_string());
            };
            if rendered_text.is_empty() {
                debug!(
                    radio_id = self.radio_id,
                    mode, key, "ignoring empty cw message"
                );
                continue;
            }
            rendered_parts.push(rendered_text);
        }
        if rendered_parts.is_empty() {
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                "ignoring empty cw message sequence"
            );
            return Ok(());
        }
        let text = cw_send_text(rendered_parts.join(" "), prepend_space);
        debug!(
            radio_id = self.radio_id,
            mode,
            ?keys,
            text,
            "sending cw text"
        );
        let completion = {
            let Some(keyer) = self.keyer.as_mut() else {
                debug!(
                    radio_id = self.radio_id,
                    "ignoring CW send; no CW keyer configured"
                );
                return Err("cw keyer unavailable".to_string());
            };
            let keyer_name = keyer.name();
            let completion = keyer.send_text(self.radio_id, &text).await?;
            debug!(
                radio_id = self.radio_id,
                mode,
                ?keys,
                keyer = keyer_name,
                "cw text queued"
            );
            completion
        };

        match completion {
            CwSendCompletion::PollStatus { wait_for_busy } => {
                if wait_for_busy {
                    debug!(
                        radio_id = self.radio_id,
                        mode,
                        ?keys,
                        "waiting for cw keyer busy"
                    );
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    "waiting for cw keyer idle"
                );
                let result = self.wait_until_idle_or_stopped(commands, pending).await;
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    ?result,
                    "finished waiting for cw keyer idle"
                );
                result
            }
            CwSendCompletion::RadioCatUpdates(updates) => {
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    ?keys,
                    "waiting for radio-cat cw completion updates"
                );
                self.wait_until_radio_cat_cw_complete_or_stopped(updates, commands, pending)
                    .await
            }
        }
    }

    async fn send_voice_messages(
        &mut self,
        mode: &str,
        keys: &[String],
        fields: &serde_json::Map<String, serde_json::Value>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        for key in keys {
            let Some(file_path) = voice_messages::file_path_for(&self.voice_messages, mode, key)
            else {
                debug!(
                    radio_id = self.radio_id,
                    mode, key, "ignoring voice message without a file"
                );
                continue;
            };
            let Some(voice_playback) = self.voice_playback.as_ref() else {
                return Err("voice keyer thread unavailable".to_string());
            };

            let mut data_ptt = VoiceDataPttGuard::acquire(
                self.radio_id,
                self.radio.clone(),
                self.voice_data_ptt_supported,
            )
            .await;
            let completed = if voice_messages::file_path_has_template(&file_path) {
                let resolved = voice_messages::resolved_file_path_for(
                    &self.voice_messages,
                    mode,
                    key,
                    fields,
                )?
                .ok_or_else(|| "voice message without a file".to_string())?;
                debug!(
                    radio_id = self.radio_id,
                    mode,
                    key,
                    configured_voice_file = %file_path,
                    resolved_voice_file = %resolved,
                    operator = ?fields.get("OPERATOR"),
                    station_callsign = ?fields.get("STATION_CALLSIGN"),
                    ?fields,
                    "resolved templated voice message file path"
                );
                match voice_playback.play_file_path(&resolved) {
                    Ok(completed) => completed,
                    Err(error) => {
                        warn!(
                            radio_id = self.radio_id,
                            mode,
                            key,
                            configured_voice_file = %file_path,
                            resolved_voice_file = %resolved,
                            operator = ?fields.get("OPERATOR"),
                            station_callsign = ?fields.get("STATION_CALLSIGN"),
                            ?fields,
                            %error,
                            "failed to play templated voice message file"
                        );
                        data_ptt.release().await;
                        return Err(error);
                    }
                }
            } else {
                match voice_playback.play_message(mode, key) {
                    Ok(completed) => completed,
                    Err(error) => {
                        data_ptt.release().await;
                        return Err(error);
                    }
                }
            };
            debug!(
                radio_id = self.radio_id,
                mode, key, "queued voice keyer playback"
            );
            let result = self
                .wait_until_voice_playback_done_or_stopped(key, completed, commands, pending)
                .await;
            data_ptt.release().await;
            result?;
        }

        Ok(())
    }

    async fn wait_until_voice_playback_done_or_stopped(
        &mut self,
        key: &str,
        mut completed: tokio::sync::oneshot::Receiver<Result<Duration, String>>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                result = &mut completed => {
                    return match result {
                        Ok(Ok(duration)) => {
                            debug!(radio_id = self.radio_id, key, duration_ms = duration.as_millis(), "voice keyer playback completed");
                            Ok(())
                        }
                        Ok(Err(error)) => Err(error),
                        Err(_) => Err("voice keyer thread unavailable".to_string()),
                    };
                }
                command = commands.recv() => {
                    self.apply_wait_command(
                        command,
                        pending,
                        WaitContext::new("voice keyer playback", "keying stopped", "keying shutdown"),
                    ).await?;
                }
            }
        }
    }

    fn radio_logger_mode(&self) -> Option<String> {
        match self.radio.latest_state().main_rx.mode {
            Some(mode) => Some(crate::radio::normalize_mode(&mode)),
            None => {
                warn!(
                    radio_id = self.radio_id,
                    "radio mode is unknown before message send"
                );
                None
            }
        }
    }

    async fn send_text(
        &mut self,
        text: &str,
        wait_for_completion: bool,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        if text.trim().is_empty() {
            return Ok(());
        }

        debug!(radio_id = self.radio_id, text, "sending cw text");
        let completion = {
            let Some(keyer) = self.keyer.as_mut() else {
                debug!(
                    radio_id = self.radio_id,
                    "ignoring CW send; no CW keyer configured"
                );
                return Err("cw keyer unavailable".to_string());
            };
            let keyer_name = keyer.name();
            let completion = keyer.send_text(self.radio_id, text).await?;
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                "cw text queued"
            );
            completion
        };

        if !wait_for_completion {
            debug!(
                radio_id = self.radio_id,
                text, "cw text queued without waiting for completion"
            );
            return Ok(());
        }

        match completion {
            CwSendCompletion::PollStatus { wait_for_busy } => {
                if wait_for_busy {
                    self.wait_until_busy_or_stopped(commands, pending).await?;
                }
                self.wait_until_idle_or_stopped(commands, pending).await
            }
            CwSendCompletion::RadioCatUpdates(updates) => {
                self.wait_until_radio_cat_cw_complete_or_stopped(updates, commands, pending)
                    .await
            }
        }
    }

    async fn wait_until_busy_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    self.apply_wait_command(
                        command,
                        pending,
                        WaitContext::new("cw busy wait", "cw stopped", "cw shutdown"),
                    ).await?;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    warn!(radio_id = self.radio_id, "timed out waiting for cw keyer to become busy");
                    return Err("cw keyer did not become busy".to_string());
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    match self.keyer_status().await {
                        Ok(status) if status.busy => {
                            debug!(radio_id = self.radio_id, "cw keyer is busy");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id = self.radio_id, %error, "failed waiting for cw keyer busy");
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_idle_or_stopped(
        &mut self,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    self.apply_wait_command(
                        command,
                        pending,
                        WaitContext::new("cw idle wait", "cw stopped", "cw shutdown"),
                    ).await?;
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    match self.keyer_status().await {
                        Ok(status) if !status.busy => {
                            debug!(radio_id = self.radio_id, "cw keyer is idle");
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(radio_id = self.radio_id, %error, "failed waiting for cw keyer idle");
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_radio_cat_cw_complete_or_stopped(
        &mut self,
        mut updates: broadcast::Receiver<StateUpdate>,
        commands: &mut mpsc::Receiver<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
    ) -> Result<(), String> {
        let mut saw_busy = self
            .radio
            .latest_state()
            .keyer
            .as_ref()
            .and_then(|keyer| keyer.sending)
            == Some(true);
        if saw_busy {
            debug!(
                radio_id = self.radio_id,
                "radio-cat keyer already reports sending"
            );
        }

        loop {
            tokio::select! {
                command = commands.recv() => {
                    self.apply_wait_command(
                        command,
                        pending,
                        WaitContext::new("radio-cat cw wait", "cw stopped", "cw shutdown"),
                    ).await?;
                }
                update = updates.recv() => {
                    match update {
                        Ok(update) => {
                            if !update.changes.contains(ChangeFlags::KEYER)
                                || !update.fields.contains(&StateField::KeyerSending) {
                                continue;
                            }
                            match update.state.keyer.as_ref().and_then(|keyer| keyer.sending) {
                                Some(true) => {
                                    saw_busy = true;
                                    debug!(radio_id = self.radio_id, source = ?update.source, "radio-cat keyer reports sending");
                                }
                                Some(false) => {
                                    debug!(radio_id = self.radio_id, source = ?update.source, saw_busy, "radio-cat keyer reports idle");
                                    return Ok(());
                                }
                                None => {
                                    debug!(radio_id = self.radio_id, source = ?update.source, "radio-cat keyer sending state unavailable");
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(radio_id = self.radio_id, skipped, "lagged while waiting for radio-cat cw completion updates");
                            match self.radio.latest_state().keyer.as_ref().and_then(|keyer| keyer.sending) {
                                Some(true) => {
                                    saw_busy = true;
                                    debug!(radio_id = self.radio_id, "radio-cat keyer still reports sending after lag");
                                }
                                Some(false) => {
                                    debug!(radio_id = self.radio_id, saw_busy, "radio-cat keyer reports idle after lag");
                                    return Ok(());
                                }
                                None => {}
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            warn!(radio_id = self.radio_id, "radio-cat cw completion update channel closed");
                            return Err("radio-cat cw completion updates unavailable".to_string());
                        }
                    }
                }
            }
        }
    }

    async fn apply_wait_command(
        &mut self,
        command: Option<CwTaskCommand>,
        pending: &mut VecDeque<PendingCwSend>,
        context: WaitContext,
    ) -> Result<(), String> {
        match handle_wait_command(self.radio_id, command, pending, context) {
            WaitCommandEffect::Continue => Ok(()),
            WaitCommandEffect::SetWpm(wpm) => {
                self.set_wpm(wpm).await;
                Ok(())
            }
            WaitCommandEffect::Interrupt(reason) => {
                self.stop().await;
                Err(reason.to_string())
            }
        }
    }

    async fn stop(&mut self) {
        if let Some(voice_playback) = self.voice_playback.as_ref() {
            voice_playback.stop_keying();
        }

        if let Some(keyer) = self.keyer.as_mut() {
            let keyer_name = keyer.name();
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                "clearing cw keyer buffer"
            );
            if let Err(error) = keyer.clear_buffer(self.radio_id).await {
                warn!(radio_id = self.radio_id, keyer = keyer_name, %error, "failed to clear cw keyer buffer");
            } else {
                debug!(
                    radio_id = self.radio_id,
                    keyer = keyer_name,
                    "cw keyer buffer cleared"
                );
            }
        }
    }

    async fn set_wpm(&mut self, wpm: u8) {
        if let Some(keyer) = self.keyer.as_mut() {
            let keyer_name = keyer.name();
            debug!(
                radio_id = self.radio_id,
                keyer = keyer_name,
                wpm,
                "setting cw keyer wpm"
            );
            if let Err(error) = keyer.set_wpm(self.radio_id, wpm).await {
                warn!(radio_id = self.radio_id, keyer = keyer_name, wpm, %error, "failed to set cw keyer wpm");
            } else {
                debug!(
                    radio_id = self.radio_id,
                    keyer = keyer_name,
                    wpm,
                    "cw keyer wpm set"
                );
            }
        }
    }

    async fn close(&mut self) {
        if let Some(voice_playback) = self.voice_playback.as_mut() {
            voice_playback.shutdown();
        }

        if let Some(keyer) = self.keyer.as_mut() {
            keyer.close(self.radio_id).await;
        }
    }

    async fn keyer_status(&mut self) -> Result<CwKeyerStatus, String> {
        let Some(keyer) = self.keyer.as_mut() else {
            return Err("cw keyer unavailable".to_string());
        };
        let keyer_name = keyer.name();
        keyer
            .status(self.radio_id)
            .await?
            .ok_or_else(|| format!("{keyer_name} keyer does not report status"))
    }
}

#[derive(Clone, Copy)]
struct WaitContext {
    waiting_for: &'static str,
    stop_reason: &'static str,
    shutdown_reason: &'static str,
}

impl WaitContext {
    const fn new(
        waiting_for: &'static str,
        stop_reason: &'static str,
        shutdown_reason: &'static str,
    ) -> Self {
        Self {
            waiting_for,
            stop_reason,
            shutdown_reason,
        }
    }
}

enum WaitCommandEffect {
    Continue,
    SetWpm(u8),
    Interrupt(&'static str),
}

fn handle_wait_command(
    radio_id: i64,
    command: Option<CwTaskCommand>,
    pending: &mut VecDeque<PendingCwSend>,
    context: WaitContext,
) -> WaitCommandEffect {
    match command {
        Some(CwTaskCommand::Stop) => {
            debug!(
                radio_id,
                waiting_for = context.waiting_for,
                "stop command interrupting wait"
            );
            fail_pending_cw_sends(pending, context.stop_reason);
            WaitCommandEffect::Interrupt(context.stop_reason)
        }
        Some(CwTaskCommand::SetWpm(wpm)) => {
            debug!(
                radio_id,
                wpm,
                waiting_for = context.waiting_for,
                "set_wpm command received while waiting"
            );
            WaitCommandEffect::SetWpm(wpm)
        }
        Some(CwTaskCommand::Shutdown) | None => {
            debug!(
                radio_id,
                waiting_for = context.waiting_for,
                "shutdown interrupting wait"
            );
            fail_pending_cw_sends(pending, context.shutdown_reason);
            WaitCommandEffect::Interrupt(context.shutdown_reason)
        }
        Some(CwTaskCommand::SendMessage {
            mode,
            keys,
            fields,
            completed,
        }) => {
            debug!(
                radio_id,
                mode,
                ?keys,
                waiting_for = context.waiting_for,
                pending_count = pending.len(),
                "queueing message send command while waiting"
            );
            pending.push_back(PendingCwSend {
                payload: PendingCwPayload::Message { mode, keys, fields },
                completed,
            });
            WaitCommandEffect::Continue
        }
        Some(CwTaskCommand::SendText {
            text,
            wait_for_completion,
            completed,
        }) => {
            debug!(
                radio_id,
                text,
                wait_for_completion,
                waiting_for = context.waiting_for,
                pending_count = pending.len(),
                "queueing cw text send command while waiting"
            );
            pending.push_back(PendingCwSend {
                payload: PendingCwPayload::Text {
                    text,
                    wait_for_completion,
                },
                completed,
            });
            WaitCommandEffect::Continue
        }
    }
}

fn fail_pending_cw_sends(pending: &mut VecDeque<PendingCwSend>, reason: &str) {
    while let Some(send) = pending.pop_front() {
        let _ = send.completed.send(Err(reason.to_string()));
    }
}

fn cw_send_text(text: String, prepend_space: bool) -> String {
    if prepend_space {
        format!(" {text}")
    } else {
        text
    }
}

pub(super) struct CwKeyerStatus {
    pub(super) busy: bool,
}

pub(super) enum CwSendCompletion {
    PollStatus { wait_for_busy: bool },
    RadioCatUpdates(broadcast::Receiver<StateUpdate>),
}

pub(super) trait CwKeyer: Send {
    fn name(&self) -> &'static str;
    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()>;
    fn send_text<'a>(
        &'a mut self,
        radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>>;
    fn status<'a>(
        &'a mut self,
        radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>>;
    fn clear_buffer<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, Result<(), String>>;
    fn set_wpm<'a>(&'a mut self, radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>>;
    fn close<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn pending_message(key: &str) -> (PendingCwSend, oneshot::Receiver<Result<(), String>>) {
        let (completed, received) = oneshot::channel();
        (
            PendingCwSend {
                payload: PendingCwPayload::Message {
                    mode: "run".to_string(),
                    keys: vec![key.to_string()],
                    fields: Map::new(),
                },
                completed,
            },
            received,
        )
    }

    #[tokio::test]
    async fn fail_pending_cw_sends_rejects_all_queued_requests() {
        let (first, first_rx) = pending_message("F1");
        let (second, second_rx) = pending_message("F2");
        let mut pending = VecDeque::from([first, second]);

        fail_pending_cw_sends(&mut pending, "cw stopped");

        assert!(pending.is_empty());
        assert_eq!(first_rx.await.unwrap(), Err("cw stopped".to_string()));
        assert_eq!(second_rx.await.unwrap(), Err("cw stopped".to_string()));
    }

    #[test]
    fn queued_cw_send_text_prepends_single_space() {
        assert_eq!(cw_send_text("CQ TEST".to_string(), false), "CQ TEST");
        assert_eq!(cw_send_text("CQ TEST".to_string(), true), " CQ TEST");
    }

    #[test]
    fn handle_wait_command_queues_message_sends() {
        let mut pending = VecDeque::new();
        let (completed, _rx) = oneshot::channel();

        let effect = handle_wait_command(
            7,
            Some(CwTaskCommand::SendMessage {
                mode: "run".to_string(),
                keys: vec!["F1".to_string(), "F2".to_string()],
                fields: Map::new(),
                completed,
            }),
            &mut pending,
            WaitContext::new("cw idle wait", "cw stopped", "cw shutdown"),
        );

        assert!(matches!(effect, WaitCommandEffect::Continue));
        assert_eq!(pending.len(), 1);
        let Some(PendingCwSend {
            payload: PendingCwPayload::Message { mode, keys, .. },
            ..
        }) = pending.pop_front()
        else {
            panic!("expected queued message send");
        };
        assert_eq!(mode, "run");
        assert_eq!(keys, vec!["F1", "F2"]);
    }

    #[test]
    fn handle_wait_command_queues_text_sends() {
        let mut pending = VecDeque::new();
        let (completed, _rx) = oneshot::channel();

        let effect = handle_wait_command(
            7,
            Some(CwTaskCommand::SendText {
                text: "CQ".to_string(),
                wait_for_completion: true,
                completed,
            }),
            &mut pending,
            WaitContext::new("cw busy wait", "cw stopped", "cw shutdown"),
        );

        assert!(matches!(effect, WaitCommandEffect::Continue));
        assert_eq!(pending.len(), 1);
        let Some(PendingCwSend {
            payload:
                PendingCwPayload::Text {
                    text,
                    wait_for_completion,
                },
            ..
        }) = pending.pop_front()
        else {
            panic!("expected queued text send");
        };
        assert_eq!(text, "CQ");
        assert!(wait_for_completion);
    }

    #[tokio::test]
    async fn handle_wait_command_stop_fails_pending_and_interrupts() {
        let (queued, queued_rx) = pending_message("F1");
        let mut pending = VecDeque::from([queued]);

        let effect = handle_wait_command(
            7,
            Some(CwTaskCommand::Stop),
            &mut pending,
            WaitContext::new("cw idle wait", "cw stopped", "cw shutdown"),
        );

        assert!(matches!(effect, WaitCommandEffect::Interrupt("cw stopped")));
        assert!(pending.is_empty());
        assert_eq!(queued_rx.await.unwrap(), Err("cw stopped".to_string()));
    }

    #[tokio::test]
    async fn handle_wait_command_shutdown_fails_pending_and_interrupts() {
        let (queued, queued_rx) = pending_message("F1");
        let mut pending = VecDeque::from([queued]);

        let effect = handle_wait_command(
            7,
            Some(CwTaskCommand::Shutdown),
            &mut pending,
            WaitContext::new("voice keyer playback", "keying stopped", "keying shutdown"),
        );

        assert!(matches!(
            effect,
            WaitCommandEffect::Interrupt("keying shutdown")
        ));
        assert!(pending.is_empty());
        assert_eq!(queued_rx.await.unwrap(), Err("keying shutdown".to_string()));
    }

    #[test]
    fn handle_wait_command_returns_set_wpm_without_disturbing_queue() {
        let (queued, _queued_rx) = pending_message("F1");
        let mut pending = VecDeque::from([queued]);

        let effect = handle_wait_command(
            7,
            Some(CwTaskCommand::SetWpm(28)),
            &mut pending,
            WaitContext::new("radio-cat cw wait", "cw stopped", "cw shutdown"),
        );

        assert!(matches!(effect, WaitCommandEffect::SetWpm(28)));
        assert_eq!(pending.len(), 1);
    }
}
