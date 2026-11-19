use super::*;
use threadline_storage::reader::{KnownCount, SidebarCounts};

#[gpui_kit::test]
fn ui_005_sidebar_circles_fit_the_painted_clip_at_multiple_scales(cx: &mut TestAppContext) {
    let harness = ReaderHarness::seeded();
    let (handle, reader) = harness.open(cx);
    let known = |known, complete| KnownCount {
        known,
        complete,
        last_validated_at_ms: Some(1_700_000_000_000),
    };
    let rows = [
        (
            "reader-review",
            "Review requested",
            7,
            "known from partial downloads",
        ),
        (
            "reader-assigned",
            "Assigned to me",
            42,
            "complete cached count",
        ),
        ("reader-outbox", "Outbox", 901, "complete cached count"),
    ];
    for scale in [1., 1.5, 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        cx.update_window(handle, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.counts = Some(SidebarCounts {
                    review_requested: known(7, false),
                    assigned: known(42, true),
                    outbox: 901,
                });
                cx.notify();
            });
            window.render_frame(cx);
            let mut right: Option<f32> = None;
            let quads = window.painted_quads();
            let texts = gpui_kit::base::test_support::texts(window);
            for (id, name, count, coverage) in rows {
                let button = window.find(id);
                let label = button.label().unwrap();
                assert!(label.contains(&format!("{count} {coverage}")), "{label}");
                let badge = window.find(format!("{id}-badge")).bounds();
                assert_eq!(badge.size.width, badge.size.height);
                let text = |value: &str| {
                    texts
                        .iter()
                        .find(|t| {
                            t.text() == value
                                && t.visible()
                                && button.bounds().contains(&t.bounds().center())
                        })
                        .expect("painted sidebar text inside its own button")
                        .bounds()
                };
                let digits = text(&count.to_string());
                let title = text(name);
                assert!(title.right() + px(4.) <= badge.origin.x);
                assert!((badge.center().x - digits.center().x).abs() <= px(1.));
                assert!((badge.center().y - digits.center().y).abs() <= px(1.));
                assert!(button.bounds().right() - badge.right() <= px(16.));
                let physical = badge.scale(window.scale_factor());
                let quad = quads
                    .iter()
                    .find(|q| {
                        q.background.as_solid() == Some(gpui_kit::rgb(0x173e70).into())
                            && (q.bounds.center().y.0 - physical.center().y.0).abs() <= 1.
                    })
                    .expect("painted dark blue badge at the expected position");
                let clipped = quad.bounds.intersect(&quad.content_mask.bounds);
                assert_eq!(
                    quad.bounds, clipped,
                    "badge {id} at {scale}x: clip {:?}",
                    quad.content_mask.bounds
                );
                let radius = quad.bounds.size.width.0 / 2.;
                for r in [
                    quad.corner_radii.top_left,
                    quad.corner_radii.top_right,
                    quad.corner_radii.bottom_left,
                    quad.corner_radii.bottom_right,
                ] {
                    assert!(
                        r.0 >= radius - 1.,
                        "badge {id} corner {:?} radius {radius}",
                        r
                    );
                }
                let x: f32 = badge.right().into();
                if let Some(prior) = right {
                    assert!((x - prior).abs() <= 1.);
                }
                right = Some(x);
            }
        })
        .unwrap();
    }
    for counts in [
        None,
        Some(SidebarCounts {
            review_requested: known(0, false),
            assigned: known(0, true),
            outbox: 0,
        }),
    ] {
        cx.update_window(handle, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.counts = counts;
                cx.notify();
            });
            window.render_frame(cx);
            for (id, _, _, _) in rows {
                assert!(window.try_find(format!("{id}-badge")).is_none());
                let button = window.find(id);
                let expected = if counts.is_none() {
                    "count pending"
                } else if id == "reader-review" {
                    "0 known from partial downloads"
                } else {
                    "0 complete cached count"
                };
                assert!(button.label().unwrap().contains(expected));
            }
        })
        .unwrap();
    }
}

#[test]
fn ui_005_sidebar_digits_have_sufficient_contrast_on_the_circle() {
    fn luminance(rgb: u32) -> f64 {
        let channel = |bits| {
            let srgb = f64::from((rgb >> bits) & 255_u32) / 255.;
            if srgb <= 0.04045 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
    let background = luminance(view::BADGE_BACKGROUND);
    let digits = luminance(view::BADGE_FOREGROUND);
    assert!(digits > background);
    assert!((digits + 0.05) / (background + 0.05) >= 4.5);
}
