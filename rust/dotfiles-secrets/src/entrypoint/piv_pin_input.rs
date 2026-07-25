//! PIV PIN の prompt、入力順序、confirmation を所有する CLI input boundary。

use crate::{
    Result,
    composition::HiddenTtySecretReader,
    domain::piv::validate_piv_pin_properties,
    ports::{PivPinInputPort, ProtectedSecret},
};

pub(crate) struct PivPinInputBoundary {
    read_hidden_tty_secret: HiddenTtySecretReader,
}

impl PivPinInputBoundary {
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

impl PivPinInputPort for PivPinInputBoundary {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.read_with_prompt("YubiKey PIV PIN: ")
    }

    fn read_current_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        self.read_with_prompt("YubiKey current PIV PIN: ")
    }

    fn read_new_piv_pin_confirmation(&self) -> Result<ProtectedSecret> {
        let new = self.read_with_prompt("YubiKey new PIV PIN: ")?;
        let confirmation = self.read_with_prompt("Confirm YubiKey new PIV PIN: ")?;
        if new != confirmation {
            anyhow::bail!("YubiKey PIV PIN confirmation does not match");
        }
        Ok(new)
    }

    fn read_piv_pin_change_secrets(&self) -> Result<(ProtectedSecret, ProtectedSecret)> {
        let current = self.read_current_piv_pin_secret()?;
        Ok((current, self.read_new_piv_pin_confirmation()?))
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
        let bytes = INPUTS.with(|inputs| {
            inputs
                .borrow_mut()
                .pop_front()
                .expect("fixture input must exist")
        });
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
        let boundary = PivPinInputBoundary::new(fixture_reader);

        let (current, new) = boundary.read_piv_pin_change_secrets()?;

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
        let boundary = PivPinInputBoundary::new(fixture_reader);

        let error = boundary
            .read_new_piv_pin_confirmation()
            .err()
            .expect("mismatched confirmation must fail");

        assert!(error.to_string().contains("confirmation does not match"));
    }
}
