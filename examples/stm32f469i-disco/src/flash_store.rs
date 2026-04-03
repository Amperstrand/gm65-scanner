use embassy_stm32::flash::Flash;

use crate::compatibility::{CompatibilityProfile, PROFILE_FLASH_BYTES};

pub const PROFILE_FLASH_OFFSET: u32 = 1024 * 1024;
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
