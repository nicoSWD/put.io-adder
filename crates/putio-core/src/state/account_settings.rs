//! Changing the account's settings on put.io: whether deleted files go to the trash, and where transfers
//! are saved when they are added without a folder.
//!
//! The settings come with every account poll; a change is sent on its own and, once put.io has agreed,
//! applied to what the app knows and followed by a fresh poll. Until then the switch shows the position it
//! was moved to. One change at a time: a second one waits for the first, so the answers cannot cross.

use putio_api::{Account, AccountSettingChange, AccountSettings, ApiError};

use super::{Effect, Millis, Op, OpId, State, session_expired};

/// What a change in progress, or the last one that failed, looks like.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountSettingSave {
    /// The change put.io has not answered yet.
    pub pending: Option<AccountSettingChange>,
    /// Why the last change was refused, in the user's words; gone with the next change.
    pub error: Option<String>,
}

pub(super) fn requested(state: &mut State, change: AccountSettingChange, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() || state.account_setting.pending.is_some() {
        return;
    }
    let op = state.op(Op::AccountSetting);
    state.account_setting = AccountSettingSave {
        pending: Some(change),
        error: None,
    };
    fx.push(Effect::SaveAccountSetting { op, change });
}

pub(super) fn finished(
    state: &mut State,
    now: Millis,
    (op, change): (OpId, AccountSettingChange),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::AccountSetting]) {
        return;
    }
    state.account_setting.pending = None;
    match result {
        Ok(()) => {
            let settings = state
                .account
                .get_or_insert_with(Account::default)
                .settings
                .get_or_insert_with(AccountSettings::default);
            match change {
                AccountSettingChange::DefaultDownloadFolder(id) => {
                    settings.default_download_folder = Some(id);
                }
                AccountSettingChange::TrashEnabled(on) => settings.trash_enabled = Some(on),
            }
            // The next poll shows what put.io says now.
            state.schedule_poll(now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => state.account_setting.error = Some(state.reason(&error)),
    }
}

pub(super) fn dismissed(state: &mut State) {
    state.account_setting.error = None;
}

/// Forget everything about the account's settings (signing out).
pub(super) fn reset(state: &mut State) {
    state.account_setting = AccountSettingSave::default();
}

impl State {
    /// Whether deleted files go to put.io's trash: the position a change is moving to while it is sent, else
    /// what put.io said. `None` when it did not say.
    #[must_use]
    pub fn trash_enabled(&self) -> Option<bool> {
        match self.account_setting.pending {
            Some(AccountSettingChange::TrashEnabled(on)) => Some(on),
            _ => self.account.as_ref()?.settings?.trash_enabled,
        }
    }

    /// The folder put.io saves transfers in when they are added without one (`0` is the top level), with a
    /// change in progress taken into account. `None` when put.io did not say.
    #[must_use]
    pub fn account_default_folder(&self) -> Option<i64> {
        match self.account_setting.pending {
            Some(AccountSettingChange::DefaultDownloadFolder(id)) => Some(id),
            _ => self.account.as_ref()?.settings?.default_download_folder,
        }
    }
}

#[cfg(test)]
mod tests {
    use putio_api::{ApiFailure, Transfer};

    use super::super::{Auth, Event};
    use super::*;
    use crate::i18n::Locale;
    use crate::settings::Settings;
    use crate::state::update;

    fn signed_in(settings: Option<AccountSettings>) -> State {
        let settings_file = Settings {
            check_for_updates: false,
            ..Settings::default()
        };
        let mut state = State::new(settings_file, Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: true });
        update(
            &mut state,
            0,
            Event::Polled {
                account: Ok(Account {
                    settings,
                    ..Account::default()
                }),
                transfers: Ok(Vec::<Transfer>::new()),
            },
        );
        assert_eq!(state.auth, Auth::SignedIn);
        state
    }

    fn known() -> AccountSettings {
        AccountSettings {
            default_download_folder: Some(0),
            trash_enabled: Some(true),
        }
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::SaveAccountSetting { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a save was asked for")
    }

    fn refused(status: u16) -> ApiError {
        ApiError::Api(ApiFailure {
            status,
            error_type: Some("INVALID_VALUE".into()),
            message: "nope".into(),
        })
    }

    #[test]
    fn the_settings_come_with_the_account() {
        let state = signed_in(Some(known()));
        assert_eq!(state.trash_enabled(), Some(true));
        assert_eq!(state.account_default_folder(), Some(0));
        // What put.io did not say is not made up.
        let state = signed_in(None);
        assert_eq!(
            (state.trash_enabled(), state.account_default_folder()),
            (None, None)
        );
    }

    #[test]
    fn a_change_shows_at_once_and_stays_when_put_io_agrees() {
        let mut state = signed_in(Some(known()));
        let change = AccountSettingChange::TrashEnabled(false);
        let fx = update(&mut state, 100, Event::AccountSettingRequested(change));
        let op = op_of(&fx);
        assert!(fx.contains(&Effect::SaveAccountSetting { op, change }));
        assert_eq!(state.trash_enabled(), Some(false), "moved while it is sent");
        let fx = update(
            &mut state,
            150,
            Event::AccountSettingSaved {
                op,
                change,
                result: Ok(()),
            },
        );
        assert_eq!(state.account_setting, AccountSettingSave::default());
        assert_eq!(state.trash_enabled(), Some(false));
        // And a fresh poll is asked for, so that what put.io says replaces what was assumed.
        assert!(fx.contains(&Effect::Poll), "{fx:?}");
    }

    #[test]
    fn a_refused_change_goes_back_and_says_why() {
        let mut state = signed_in(Some(known()));
        let change = AccountSettingChange::DefaultDownloadFolder(9);
        let op = op_of(&update(
            &mut state,
            100,
            Event::AccountSettingRequested(change),
        ));
        assert_eq!(state.account_default_folder(), Some(9));
        update(
            &mut state,
            150,
            Event::AccountSettingSaved {
                op,
                change,
                result: Err(refused(400)),
            },
        );
        assert_eq!(state.account_default_folder(), Some(0), "back where it was");
        assert!(state.account_setting.pending.is_none());
        assert!(state.account_setting.error.is_some());
        // The next change starts clean, and the message can be put away.
        let op = op_of(&update(
            &mut state,
            200,
            Event::AccountSettingRequested(AccountSettingChange::TrashEnabled(false)),
        ));
        assert!(state.account_setting.error.is_none());
        update(
            &mut state,
            210,
            Event::AccountSettingSaved {
                op,
                change: AccountSettingChange::TrashEnabled(false),
                result: Err(ApiError::Network("x".into())),
            },
        );
        assert!(state.account_setting.error.is_some());
        update(&mut state, 220, Event::AccountSettingDismissed);
        assert!(state.account_setting.error.is_none());
    }

    #[test]
    fn only_one_change_is_sent_at_a_time() {
        let mut state = signed_in(Some(known()));
        let first = AccountSettingChange::TrashEnabled(false);
        update(&mut state, 100, Event::AccountSettingRequested(first));
        let fx = update(
            &mut state,
            101,
            Event::AccountSettingRequested(AccountSettingChange::DefaultDownloadFolder(5)),
        );
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::SaveAccountSetting { .. })),
            "{fx:?}"
        );
        assert_eq!(state.account_setting.pending, Some(first));
    }

    #[test]
    fn a_stale_answer_changes_nothing_and_an_expired_session_signs_out() {
        let mut state = signed_in(Some(known()));
        let change = AccountSettingChange::TrashEnabled(false);
        update(
            &mut state,
            100,
            Event::AccountSettingSaved {
                op: OpId(999),
                change,
                result: Ok(()),
            },
        );
        assert_eq!(state.trash_enabled(), Some(true));
        let op = op_of(&update(
            &mut state,
            110,
            Event::AccountSettingRequested(change),
        ));
        let fx = update(
            &mut state,
            120,
            Event::AccountSettingSaved {
                op,
                change,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert_eq!(state.account_setting, AccountSettingSave::default());
    }

    #[test]
    fn nothing_is_sent_when_signed_out() {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        let fx = update(
            &mut state,
            0,
            Event::AccountSettingRequested(AccountSettingChange::TrashEnabled(true)),
        );
        assert!(fx.is_empty(), "{fx:?}");
    }
}
