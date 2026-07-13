/// Deterministic animation clock.
///
/// The UI derives every animated glyph from the current frame number, and the
/// frame only advances when the application feeds a tick. Tests can therefore
/// step time explicitly and render identical scenes, and reduced-motion mode
/// simply stops feeding ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Animation {
    frame: u64,
}

impl Animation {
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::Animation;

    #[test]
    fn frames_advance_only_on_tick() {
        let mut animation = Animation::default();
        assert_eq!(animation.frame(), 0);
        animation.tick();
        animation.tick();
        assert_eq!(animation.frame(), 2);
    }
}
