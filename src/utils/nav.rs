//! Story 14-1 (#547, UX-DR38) — the single source of the navigation
//! entries. Before this story `components/nav_bar.html` carried two
//! hand-copied link lists (desktop strip + mobile panel) with the role
//! gates repeated in each. The entries — href, label, role gate,
//! current-page marker, Proposals badge — are now built once here and
//! rendered by `components/nav_entries.html` wherever a navigation shows
//! (the sidebar at ≥ `lg`, the small-screen panel below; Story 14-2's
//! drawer reuses the same include).
//!
//! The groups follow the approved mock-up
//! (`_bmad-output/planning-artifacts/ux-sidebar-mockup.html`):
//! Home · Catalog, Locations, Series · (Librarian+) Borrowers, Loans,
//! Wish list, Labels, Proposals · (Admin) Admin. An empty group is never
//! emitted, so a template separator between groups never stands alone.

/// One navigation entry. `key` names the icon (`components/nav_icons.html`)
/// and is the `current_page` slug that marks the entry current.
pub struct NavEntry {
    pub key: &'static str,
    pub href: &'static str,
    pub label: String,
    /// `aria-current="page"` (plus the non-colour marker) when true.
    pub current: bool,
    /// Proposals only: the badge text (`99+` cap) — see
    /// [`super::proposals_badge_text`].
    pub badge: Option<String>,
    /// Proposals only: the accessible name with the exact count — see
    /// [`super::proposals_badge_aria`].
    pub aria_label: Option<String>,
}

/// The tooltip text of a collapsed entry: the label, then the badge in
/// parentheses when one shows (the mock-up's "Proposals (12)").
impl NavEntry {
    pub fn tooltip(&self) -> String {
        match &self.badge {
            Some(badge) => format!("{} ({badge})", self.label),
            None => self.label.clone(),
        }
    }
}

/// The navigation of `role` (`Role`'s `Display` form: `anonymous`,
/// `librarian`, `admin`), grouped. An unknown role string fails closed to
/// the anonymous set. `pending_proposals` feeds the Proposals badge
/// (`None` = not counted).
pub fn nav_groups(
    role: &str,
    current_page: &str,
    locale: &str,
    pending_proposals: Option<u64>,
) -> Vec<Vec<NavEntry>> {
    let entry = |key: &'static str, href: &'static str, label_key: &str| NavEntry {
        key,
        href,
        label: rust_i18n::t!(label_key, locale = locale).to_string(),
        current: current_page == key,
        badge: None,
        aria_label: None,
    };

    let mut groups = vec![
        vec![entry("home", "/", "nav.home")],
        vec![
            entry("catalog", "/catalog", "nav.catalog"),
            entry("locations", "/locations", "nav.locations"),
            entry("series", "/series", "nav.series"),
        ],
    ];

    let librarian_plus = role == "librarian" || role == "admin";
    if librarian_plus {
        // CR #443 — labels are internal curation: Librarian+ only, an
        // anonymous visitor must not learn the feature exists.
        // Story 11-21 (FR143, UX-DR32) — Proposals carries the pending
        // count: the badge is text (never colour alone) and the entry's
        // accessible name states the exact count.
        let mut proposals = entry("proposals", "/proposals", "nav.proposals");
        proposals.badge = super::proposals_badge_text(pending_proposals);
        proposals.aria_label = super::proposals_badge_aria(pending_proposals, locale);
        groups.push(vec![
            entry("borrowers", "/borrowers", "nav.borrowers"),
            entry("loans", "/loans", "nav.loans"),
            entry("wishlist", "/wishlist", "nav.wishlist"),
            entry("labels", "/labels", "labels.nav"),
            proposals,
        ]);
    }
    if role == "admin" {
        groups.push(vec![entry("admin", "/admin", "nav.admin")]);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrefs(groups: &[Vec<NavEntry>]) -> Vec<Vec<&'static str>> {
        groups
            .iter()
            .map(|g| g.iter().map(|e| e.href).collect())
            .collect()
    }

    #[test]
    fn anonymous_sees_home_and_the_browsing_group_only() {
        let groups = nav_groups("anonymous", "home", "en", None);
        assert_eq!(
            hrefs(&groups),
            vec![vec!["/"], vec!["/catalog", "/locations", "/series"]]
        );
    }

    #[test]
    fn librarian_adds_the_management_group_in_mock_up_order() {
        let groups = nav_groups("librarian", "home", "en", None);
        assert_eq!(
            hrefs(&groups),
            vec![
                vec!["/"],
                vec!["/catalog", "/locations", "/series"],
                vec!["/borrowers", "/loans", "/wishlist", "/labels", "/proposals"],
            ]
        );
    }

    #[test]
    fn admin_adds_the_admin_group_last() {
        let groups = nav_groups("admin", "home", "en", None);
        assert_eq!(groups.len(), 4);
        assert_eq!(hrefs(&groups)[3], vec!["/admin"]);
    }

    #[test]
    fn an_unknown_role_fails_closed_to_the_anonymous_set() {
        let groups = nav_groups("superuser", "home", "en", Some(3));
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().flatten().all(|e| e.badge.is_none()));
    }

    #[test]
    fn exactly_the_current_page_entry_is_current() {
        for (page, href) in [
            ("home", "/"),
            ("catalog", "/catalog"),
            ("labels", "/labels"),
            ("proposals", "/proposals"),
            ("admin", "/admin"),
        ] {
            let groups = nav_groups("admin", page, "en", None);
            let current: Vec<&str> = groups
                .iter()
                .flatten()
                .filter(|e| e.current)
                .map(|e| e.href)
                .collect();
            assert_eq!(current, vec![href], "{page}");
        }
        // A page with no entry of its own (title detail) marks none.
        let groups = nav_groups("admin", "title", "en", None);
        assert!(groups.iter().flatten().all(|e| !e.current));
    }

    #[test]
    fn labels_are_localized_with_the_new_home_key() {
        let fr = nav_groups("admin", "home", "fr", None);
        assert_eq!(fr[0][0].label, "Accueil");
        assert_eq!(fr[1][0].label, "Catalogue");
        let de = nav_groups("admin", "home", "de", None);
        assert_eq!(de[0][0].label, "Startseite");
        assert_eq!(de[3][0].label, "Verwaltung");
    }

    #[test]
    fn proposals_badge_and_accessible_name_follow_the_count() {
        let groups = nav_groups("librarian", "home", "en", Some(150));
        let proposals = groups[2].last().unwrap();
        assert_eq!(proposals.key, "proposals");
        assert_eq!(proposals.badge.as_deref(), Some("99+"));
        assert_eq!(
            proposals.aria_label.as_deref(),
            Some("Proposals, 150 pending")
        );
        assert_eq!(proposals.tooltip(), "Proposals (99+)");

        let groups = nav_groups("librarian", "home", "en", Some(0));
        let proposals = groups[2].last().unwrap();
        assert!(proposals.badge.is_none() && proposals.aria_label.is_none());
        assert_eq!(proposals.tooltip(), "Proposals");
    }
}
