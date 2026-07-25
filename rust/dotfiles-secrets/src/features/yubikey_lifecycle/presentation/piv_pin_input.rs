//! PIV PIN の固定 prompt、入力順序、confirmation を所有する presentation boundary。
//!
//! controlling TTY の byte I/O は composition から注入された generic reader へ委譲し、この
//! module は feature 固有の文言と PIV PIN の presentation validation だけを所有する。

use crate::{
    Result, features::yubikey_lifecycle::domain::piv::validate_piv_pin_properties,
    foundation::protection::ProtectedSecret,
};

pub(crate) type HiddenTtySecretReader =
    fn(&str, usize, &'static str) -> crate::Result<ProtectedSecret>;

/// controlling TTY から PIV PIN を読む presentation-owned port receiver。
pub(crate) struct TerminalPivPinInput {
    read_hidden_tty_secret: HiddenTtySecretReader,
}

impl TerminalPivPinInput {
    pub(crate) fn new(read_hidden_tty_secret: HiddenTtySecretReader) -> Self {
        Self {
            read_hidden_tty_secret,
        }
    }

    fn read_with_prompt(&self, prompt: &str) -> Result<ProtectedSecret> {
        let pin = (self.read_hidden_tty_secret)(
            prompt,
            8,
            "YubiKey PIV PIN must contain 6 to 8 ASCII alphanumeric bytes",
        )?;
        validate_piv_pin_properties(pin.len(), pin.is_ascii_alphanumeric())?;
        Ok(pin)
    }
}

impl TerminalPivPinInput {
    /// 現在の PIN を PIV management input として読む presentation operation。
    pub(crate) fn read_current_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.read_with_prompt("YubiKey current PIV PIN: ")
    }

    /// 通常の PIV management PIN を読む presentation operation。
    pub(crate) fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.read_with_prompt("YubiKey PIV PIN: ")
    }

    /// fresh enrollment 用の new/confirmation を表示順に読み、一致を確定する。
    pub(crate) fn read_new_piv_pin_confirmation(&self) -> Result<ProtectedSecret> {
        let new = self.read_with_prompt("YubiKey new PIV PIN: ")?;
        let confirmation = self.read_with_prompt("Confirm YubiKey new PIV PIN: ")?;
        if new != confirmation {
            anyhow::bail!("YubiKey PIV PIN confirmation does not match");
        }
        Ok(new)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;

    thread_local! {
        static INPUTS: RefCell<VecDeque<Vec<u8>>> = const { RefCell::new(VecDeque::new()) };
        static PROMPTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn fixture_reader(
        prompt: &str,
        _max_len: usize,
        _too_long_message: &'static str,
    ) -> Result<ProtectedSecret> {
        PROMPTS.with(|prompts| prompts.borrow_mut().push(prompt.to_owned()));
        let bytes = INPUTS.with(|inputs| inputs.borrow_mut().pop_front());
        let bytes = bytes.ok_or_else(|| anyhow::anyhow!("fixture input must exist"))?;
        ProtectedSecret::from_test_bytes(&bytes)
    }

    fn prepare(inputs: &[&[u8]]) {
        INPUTS.with(|queue| {
            *queue.borrow_mut() = inputs.iter().map(|bytes| bytes.to_vec()).collect();
        });
        PROMPTS.with(|prompts| prompts.borrow_mut().clear());
    }

    #[test]
    fn pin_change_reads_current_new_confirmation_in_fixed_order() -> Result<()> {
        prepare(&[b"123456", b"654321", b"654321"]);
        let boundary = TerminalPivPinInput::new(fixture_reader);

        let current = boundary.read_current_piv_pin_secret()?;
        let new = boundary.read_new_piv_pin_confirmation()?;

        assert_eq!(current.to_test_bytes(), b"123456");
        assert_eq!(new.to_test_bytes(), b"654321");
        PROMPTS.with(|prompts| {
            assert_eq!(
                prompts.borrow().as_slice(),
                [
                    "YubiKey current PIV PIN: ",
                    "YubiKey new PIV PIN: ",
                    "Confirm YubiKey new PIV PIN: ",
                ]
            );
        });
        Ok(())
    }

    #[test]
    fn pin_confirmation_mismatch_is_rejected_at_input_boundary() {
        prepare(&[b"654321", b"123456"]);
        let boundary = TerminalPivPinInput::new(fixture_reader);

        let error = boundary.read_new_piv_pin_confirmation().err();

        assert!(error.is_some());
        assert!(
            error
                .as_ref()
                .is_some_and(|value| value.to_string().contains("confirmation does not match"))
        );
    }
}
