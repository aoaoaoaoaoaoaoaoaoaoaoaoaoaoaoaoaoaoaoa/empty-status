use strum::IntoEnumIterator;

pub trait RotateEnum: Sized + PartialEq + Copy {
    fn next(self) -> Self;
}

impl<T> RotateEnum for T
where
    T: IntoEnumIterator + PartialEq + Copy,
{
    fn next(self) -> Self {
        let mut it = T::iter();
        while let Some(v) = it.next() {
            if v == self {
                return it.next().unwrap_or_else(|| T::iter().next().unwrap());
            }
        }
        unreachable!("`self` must be one of the variants");
    }
}

#[macro_export]
macro_rules! impl_handle_click_nop {
    () => {
        fn handle_click(&mut self, _click: $crate::core::ClickEvent) {}
    };
}

#[macro_export]
macro_rules! impl_handle_click_rotate_mode {
    () => {
        fn handle_click(&mut self, _click: $crate::core::ClickEvent) {
            self.mode = self.mode.next();
        }
    };
}

#[macro_export]
macro_rules! mode_enum {
    ( $($member:ident),* $(,)? ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, strum_macros::EnumIter)]
        pub enum DisplayMode {
            $($member),*
        }
    };
}
