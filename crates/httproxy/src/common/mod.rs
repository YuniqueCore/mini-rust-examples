#[macro_export]
macro_rules! impl_deref_mut {
    (
        $struct:ident ( $target:ident )
    ) => {
        impl Deref for $struct {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $struct {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}
