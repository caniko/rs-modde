pub mod inline;
pub mod kdiff3;
pub mod meld;
pub mod vscode;

use super::MergeDriver;

pub use inline::InlineDriver;
pub use kdiff3::KDiff3Driver;
pub use meld::MeldDriver;
pub use vscode::VSCodeDriver;

static VSCODE: VSCodeDriver = VSCodeDriver;
static MELD: MeldDriver = MeldDriver;
static KDIFF3: KDiff3Driver = KDiff3Driver;
static INLINE: InlineDriver = InlineDriver;

/// Built-in drivers in preferred discovery order.
#[must_use]
pub fn available_drivers() -> Vec<&'static dyn MergeDriver> {
    [&VSCODE as &dyn MergeDriver, &MELD, &KDIFF3, &INLINE]
        .into_iter()
        .filter(|driver| driver.is_available())
        .collect()
}

/// All built-in drivers in documented priority order.
#[must_use]
pub fn all_drivers() -> Vec<&'static dyn MergeDriver> {
    vec![&VSCODE, &MELD, &KDIFF3, &INLINE]
}

#[must_use]
pub fn driver_by_id(id: &str) -> Option<&'static dyn MergeDriver> {
    all_drivers().into_iter().find(|driver| driver.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_drivers_has_documented_order() {
        assert_eq!(
            all_drivers()
                .into_iter()
                .map(MergeDriver::id)
                .collect::<Vec<_>>(),
            vec!["vscode", "meld", "kdiff3", "inline"]
        );
    }
}
