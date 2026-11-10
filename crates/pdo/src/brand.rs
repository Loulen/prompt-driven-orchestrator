//! The Marque of the binary: the name and the logo the interface shows. Declared
//! at compile time, like the release channel (ADR draft « la Marque est déclarée
//! par le binaire à la compilation »): the vanilla `pdo` is **PDO**, a
//! distribution may declare **Acogni Driver** with [`Config::with_brand`]. No
//! environment variable, setting or extension changes it.
//!
//! The daemon writes it into the page it serves (the title, and the
//! `data-brand` the front reads before its first render, so no wrong name
//! flashes) and serves its logos under `/shared/brand/`: `mark.svg` and
//! `lockup.svg` drawn in `currentColor` with their accent in
//! `var(--primary)`, to inline; `favicon.svg` in fixed colours that follow
//! `prefers-color-scheme`.
//!
//! [`Config::with_brand`]: crate::Config::with_brand

/// The Marques the core knows. Closed: a third one is a change of the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Brand {
    /// The open source core, the vanilla binary's.
    #[default]
    Pdo,
    /// The enterprise distribution of Acogni.
    AcogniDriver,
}

impl Brand {
    /// The name the interface shows: the window title, the product in a sentence.
    pub fn name(self) -> &'static str {
        match self {
            Brand::Pdo => "PDO",
            Brand::AcogniDriver => "Acogni Driver",
        }
    }

    /// The id the page carries in `data-brand`, which the front reads.
    pub fn id(self) -> &'static str {
        match self {
            Brand::Pdo => "pdo",
            Brand::AcogniDriver => "acogni-driver",
        }
    }

    /// The logo file `name` of this Marque (`mark.svg`, `lockup.svg`,
    /// `favicon.svg`), `None` for any other name.
    pub(crate) fn asset(self, name: &str) -> Option<&'static str> {
        let [mark, lockup, favicon] = match self {
            Brand::Pdo => [
                include_str!("../assets/brand/pdo/mark.svg"),
                include_str!("../assets/brand/pdo/lockup.svg"),
                include_str!("../assets/brand/pdo/favicon.svg"),
            ],
            Brand::AcogniDriver => [
                include_str!("../assets/brand/acogni-driver/mark.svg"),
                include_str!("../assets/brand/acogni-driver/lockup.svg"),
                include_str!("../assets/brand/acogni-driver/favicon.svg"),
            ],
        };
        match name {
            "mark.svg" => Some(mark),
            "lockup.svg" => Some(lockup),
            "favicon.svg" => Some(favicon),
            _ => None,
        }
    }

    /// `page` (the front's `index.html`) under this Marque: its title and its
    /// `data-brand`. The page ships with PDO's (`<title>PDO</title>`,
    /// `data-brand="pdo"`), which the Vite dev server serves as is.
    pub(crate) fn stamp(self, page: &str) -> String {
        page.replacen(
            &format!("<title>{}</title>", Brand::Pdo.name()),
            &format!("<title>{}</title>", self.name()),
            1,
        )
        .replacen(
            &format!("data-brand=\"{}\"", Brand::Pdo.id()),
            &format!("data-brand=\"{}\"", self.id()),
            1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page the front ships, read from the source.
    const INDEX: &str = include_str!("../../../frontend/index.html");

    #[test]
    fn the_vanilla_brand_is_pdo() {
        assert_eq!(Brand::default(), Brand::Pdo);
        assert_eq!(Brand::Pdo.name(), "PDO");
        assert_eq!(Brand::AcogniDriver.name(), "Acogni Driver");
    }

    #[test]
    fn the_shipped_page_carries_the_anchors_the_daemon_rewrites() {
        assert!(INDEX.contains("<title>PDO</title>"), "{INDEX}");
        assert!(INDEX.contains("data-brand=\"pdo\""), "{INDEX}");
        assert!(
            INDEX.contains("href=\"/shared/brand/favicon.svg\""),
            "{INDEX}"
        );
        assert_eq!(Brand::Pdo.stamp(INDEX), INDEX);
    }

    #[test]
    fn acogni_driver_rewrites_the_title_and_the_data_brand() {
        let page = Brand::AcogniDriver.stamp(INDEX);
        assert!(page.contains("<title>Acogni Driver</title>"));
        assert!(page.contains("data-brand=\"acogni-driver\""));
        assert!(!page.contains("<title>PDO</title>"));
    }

    #[test]
    fn each_brand_has_its_three_logos_and_nothing_else() {
        for brand in [Brand::Pdo, Brand::AcogniDriver] {
            for name in ["mark.svg", "lockup.svg"] {
                let svg = brand.asset(name).unwrap();
                assert!(svg.starts_with("<svg"), "{brand:?} {name}");
                assert!(
                    svg.contains(&format!("aria-label=\"{}\"", brand.name())),
                    "{brand:?} {name}"
                );
                assert!(svg.contains("var(--primary"), "{brand:?} {name}");
            }
            let favicon = brand.asset("favicon.svg").unwrap();
            assert!(favicon.contains("prefers-color-scheme: dark"), "{brand:?}");
            assert_eq!(brand.asset("../index.html"), None);
            assert_eq!(brand.asset("logo.png"), None);
        }
    }

    #[test]
    fn the_acogni_lockup_leaves_room_under_the_g() {
        let lockup = Brand::AcogniDriver.asset("lockup.svg").unwrap();
        assert!(lockup.contains("viewBox=\"-60 -1540 "), "{lockup}");
    }
}
