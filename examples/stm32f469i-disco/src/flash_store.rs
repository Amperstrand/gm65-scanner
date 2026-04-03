use embassy_stm32::flash::Flash;

use crate::compatibility::{CompatibilityProfile, PROFILE_FLASH_BYTES};

// STM32F469 internal flash is split into two 1 MiB banks. This store uses the
// start of bank 2 at 0x0810_0000 (offset 1 MiB) so profile writes stay out of
// the bank-1 application image that normally starts at 0x0800_0000.
pub const PROFILE_FLASH_OFFSET: u32 = 1024 * 1024;
// Bank-2 sector 12 is 128 KiB on STM32F469, so saves erase/rewrite the entire
// sector containing the profile blob.
pub const PROFILE_FLASH_ERASE_SIZE: u32 = 128 * 1024;

pub struct FlashStore<'d> {
    flash: Flash<'d>,
}

impl<'d> FlashStore<'d> {
    pub fn new(flash: Flash<'d>) -> Self {
        Self { flash }
    }

    pub fn load_blocking(&mut self) -> CompatibilityProfile {
        let mut buf = [0xFFu8; PROFILE_FLASH_BYTES];
        if self
            .flash
            .blocking_read(PROFILE_FLASH_OFFSET, &mut buf)
            .is_ok()
        {
            CompatibilityProfile::deserialize(&buf).unwrap_or_default()
        } else {
            CompatibilityProfile::default()
        }
    }

    pub async fn save(
        &mut self,
        profile: CompatibilityProfile,
    ) -> Result<(), embassy_stm32::flash::Error> {
        self.flash
            .erase(
                PROFILE_FLASH_OFFSET,
                PROFILE_FLASH_OFFSET + PROFILE_FLASH_ERASE_SIZE,
            )
            .await?;
        let bytes = profile.serialize();
        self.flash.write(PROFILE_FLASH_OFFSET, &bytes).await
    }
}
