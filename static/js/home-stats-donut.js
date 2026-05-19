// CR #265 — instantiate a Chart.js v4 doughnut for the catalog's
// stats-by-genre section.
//
// Reads the JSON payload from `#stats-by-genre-canvas-wrap`'s
// `data-stats-json` attribute, renders a doughnut on the
// `#stats-by-genre-canvas`, and wires slice / legend clicks to the
// existing `/?filter=genre:N` filter route. CSP-clean: no inline
// styles, no inline scripts, no eval.
//
// Chart.js vendored at `/static/js/vendor/chart.umd.js` (v4.4.7 UMD
// build, SHA256 206b6e8bb00fc7bba2c7ee80ca41db3e9e05ba7be0aa35abeba9cfd5357f5d0e).

(function () {
    "use strict";

    function init() {
        var wrap = document.getElementById("stats-by-genre-canvas-wrap");
        var canvas = document.getElementById("stats-by-genre-canvas");
        if (!wrap || !canvas) return;
        if (typeof window.Chart === "undefined") {
            // Chart.js failed to load (network blocked? vendor missing?).
            // The server-rendered legend + aria summary remain — this is
            // the graceful-degradation path the issue called out.
            return;
        }

        var raw = wrap.getAttribute("data-stats-json");
        if (!raw) return;
        var payload;
        try {
            payload = JSON.parse(raw);
        } catch (_e) {
            return;
        }

        var slices = Array.isArray(payload.slices) ? payload.slices : [];
        var other = payload.other || null;
        var totalLabel = wrap.getAttribute("data-center-total") || String(payload.total || "");
        var centerCaption = wrap.getAttribute("data-center-caption") || "";

        // Build Chart.js data arrays. The "ids" array is parallel to
        // labels / values / colors so the click handler can map a hit
        // index back to a genre id (or `null` for the Other slice).
        var labels = slices.map(function (s) { return s.name; });
        var values = slices.map(function (s) { return s.count; });
        var colors = slices.map(function (s) { return s.color; });
        var ids = slices.map(function (s) { return s.id; });
        if (other) {
            labels.push(other.label);
            values.push(other.count);
            colors.push(other.color);
            ids.push(null);
        }

        // Custom plugin draws the total count + caption in the donut
        // center. Chart.js doesn't ship native center text — `beforeDraw`
        // is the documented hook (https://www.chartjs.org/docs/latest/
        // configuration/plugins.html).
        var centerTextPlugin = {
            id: "centerText",
            beforeDraw: function (chart) {
                var ctx = chart.ctx;
                var area = chart.chartArea;
                if (!area) return;
                var cx = (area.left + area.right) / 2;
                var cy = (area.top + area.bottom) / 2;

                ctx.save();
                ctx.textAlign = "center";
                ctx.textBaseline = "middle";

                ctx.font = "600 24px system-ui, sans-serif";
                ctx.fillStyle = chart.options._centerColor || "#1f2937";
                ctx.fillText(totalLabel, cx, cy - 8);

                ctx.font = "12px system-ui, sans-serif";
                ctx.fillStyle = chart.options._centerCaptionColor || "#6b7280";
                ctx.fillText(centerCaption, cx, cy + 12);

                ctx.restore();
            },
        };

        // Respect the prefers-color-scheme: dark contrast for the center
        // text. Stone-100 vs stone-900 mirrors the surrounding UI. The
        // plugin reads `theme` via closure so we can mutate it on the
        // OS-level dark-mode toggle without rebuilding Chart's options.
        var theme = {
            dark: window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches,
        };

        // (Plugin defined inline above; rebound here so it closes over
        // the up-to-date `theme` instead of a stale `darkMode` value.)
        centerTextPlugin.beforeDraw = function (chart) {
            var ctx = chart.ctx;
            var area = chart.chartArea;
            if (!area) return;
            var cx = (area.left + area.right) / 2;
            var cy = (area.top + area.bottom) / 2;

            ctx.save();
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";

            ctx.font = "600 24px system-ui, sans-serif";
            ctx.fillStyle = theme.dark ? "#f5f5f4" : "#1c1917";
            ctx.fillText(totalLabel, cx, cy - 8);

            ctx.font = "12px system-ui, sans-serif";
            ctx.fillStyle = theme.dark ? "#a8a29e" : "#78716c";
            ctx.fillText(centerCaption, cx, cy + 12);

            ctx.restore();
        };

        var chart = new window.Chart(canvas, {
            type: "doughnut",
            data: {
                labels: labels,
                datasets: [{
                    data: values,
                    backgroundColor: colors,
                    borderColor: theme.dark ? "#1c1917" : "#ffffff",
                    borderWidth: 2,
                }],
            },
            options: {
                responsive: true,
                maintainAspectRatio: true,
                cutout: "62%",
                plugins: {
                    // Chart.js's own legend is hidden — the server-side
                    // <ul> already renders an accessible, click-routed
                    // legend below + beside the chart.
                    legend: { display: false },
                    tooltip: {
                        callbacks: {
                            label: function (ctx) {
                                var v = ctx.parsed || 0;
                                var t = values.reduce(function (a, b) { return a + b; }, 0);
                                var pct = t > 0 ? Math.round((v / t) * 1000) / 10 : 0;
                                return ctx.label + ": " + v + " (" + pct + "%)";
                            },
                        },
                    },
                },
                onClick: function (_evt, elements) {
                    if (!elements || !elements.length) return;
                    var idx = elements[0].index;
                    var id = ids[idx];
                    if (id != null) {
                        window.location.href = "/?filter=genre:" + id;
                    }
                    // Clicks on the Other slice are inert in v1 per the
                    // issue's "default to (a) — info only" decision.
                },
            },
            plugins: [centerTextPlugin],
        });

        // Re-render when the OS color scheme flips at runtime so the
        // center text + slice borders track the surrounding UI.
        if (window.matchMedia) {
            var mq = window.matchMedia("(prefers-color-scheme: dark)");
            var listener = function (e) {
                theme.dark = e.matches;
                chart.data.datasets[0].borderColor = e.matches ? "#1c1917" : "#ffffff";
                chart.update();
            };
            if (mq.addEventListener) {
                mq.addEventListener("change", listener);
            } else if (mq.addListener) {
                mq.addListener(listener);
            }
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
