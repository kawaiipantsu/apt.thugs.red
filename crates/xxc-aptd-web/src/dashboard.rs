//! Presentation values for accessible, server-rendered traffic charts.
use serde::Deserialize;
use xxc_aptd_core::analytics::{Daily, Snapshot};

#[derive(Deserialize)]
pub struct Query {
    #[serde(default = "default_days")]
    pub days: u32,
    #[serde(default = "default_metric")]
    pub metric: String,
}
fn default_days() -> u32 {
    30
}
fn default_metric() -> String {
    "downloads".into()
}
impl Query {
    pub fn valid(&self) -> bool {
        matches!(self.days, 7 | 30 | 90)
            && matches!(
                self.metric.as_str(),
                "downloads" | "requests" | "clients" | "bytes"
            )
    }
}
pub struct Metric {
    pub name: &'static str,
    pub value: String,
    pub note: &'static str,
    pub line: String,
    pub class: &'static str,
}
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub label: String,
}
pub struct Popular {
    pub name: String,
    pub downloads: String,
    pub requests: String,
    pub bytes: String,
    pub width: f64,
}
pub struct Segment {
    pub name: String,
    pub requests: String,
    pub dash: String,
    pub offset: f64,
    pub class: &'static str,
}
pub struct View {
    pub enabled: bool,
    pub days: u32,
    pub metric: String,
    pub label: &'static str,
    pub metrics: Vec<Metric>,
    pub line: String,
    pub area: String,
    pub points: Vec<Point>,
    pub maximum: String,
    pub first: String,
    pub last: String,
    pub packages: Vec<Popular>,
    pub assets: Vec<Popular>,
    pub segments: Vec<Segment>,
    pub rows: Vec<Row>,
    pub total_requests: String,
    pub ranges: String,
    pub errors: String,
    pub interrupted: String,
    pub dropped: u64,
    pub retention: u32,
    pub public_origin: String,
}
pub struct Row {
    pub date: String,
    pub downloads: String,
    pub requests: String,
    pub clients: String,
    pub bytes: String,
}
pub fn number(value: u64) -> String {
    let s = value.to_string();
    s.chars()
        .enumerate()
        .fold(String::new(), |mut out, (i, c)| {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
            out
        })
}
pub fn bytes(value: u64) -> String {
    let mut value = value as f64;
    for unit in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if value < 1024.0 || unit == "TiB" {
            return if unit == "B" {
                format!("{value:.0} {unit}")
            } else {
                format!("{value:.1} {unit}")
            };
        }
        value /= 1024.0;
    }
    unreachable!()
}
fn value(d: &Daily, metric: &str) -> u64 {
    match metric {
        "requests" => d.counts.requests,
        "clients" => d.clients,
        "assets" => d.assets,
        "bytes" => d.counts.bytes,
        _ => d.counts.downloads,
    }
}
/// Monotone cubic Hermite tangents avoid inventing peaks between daily counts.
fn curve(points: &[(f64, f64)]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let mut line = format!("M {:.2} {:.2}", points[0].0, points[0].1);
    if points.len() < 2 {
        return line;
    }
    let slopes = points
        .windows(2)
        .map(|p| (p[1].1 - p[0].1) / (p[1].0 - p[0].0))
        .collect::<Vec<_>>();
    let mut tangents = vec![slopes[0]];
    for pair in slopes.windows(2) {
        tangents.push(if pair[0] * pair[1] <= 0.0 {
            0.0
        } else {
            2.0 * pair[0] * pair[1] / (pair[0] + pair[1])
        });
    }
    tangents.push(*slopes.last().expect("slope"));
    for i in 0..points.len() - 1 {
        let (x, y) = points[i];
        let (next_x, next_y) = points[i + 1];
        let h = (next_x - x) / 3.0;
        line.push_str(&format!(
            " C {:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}",
            x + h,
            y + tangents[i] * h,
            next_x - h,
            next_y - tangents[i + 1] * h,
            next_x,
            next_y
        ));
    }
    line
}
fn coordinates(values: &[u64], width: f64, height: f64, pad: f64) -> Vec<(f64, f64)> {
    let max = values.iter().copied().max().unwrap_or(0).max(1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                pad + i as f64 * (width - 2.0 * pad) / values.len().saturating_sub(1).max(1) as f64,
                height - pad - (*v as f64 / max) * (height - 2.0 * pad),
            )
        })
        .collect()
}
impl View {
    pub fn new(
        s: Snapshot,
        q: Query,
        enabled: bool,
        retention: u32,
        dropped: u64,
        public_origin: String,
    ) -> Self {
        let label = match q.metric.as_str() {
            "requests" => "public requests",
            "clients" => "unique clients",
            "bytes" => "bytes served",
            _ => "full package downloads",
        };
        let values = s
            .daily
            .iter()
            .map(|d| value(d, &q.metric))
            .collect::<Vec<_>>();
        let coords = coordinates(&values, 1000.0, 220.0, 20.0);
        let line = curve(&coords);
        let area = format!("{line} L 980 200 L 20 200 Z");
        let maximum = values.iter().copied().max().unwrap_or(0);
        let maximum = if q.metric == "bytes" {
            bytes(maximum)
        } else {
            number(maximum)
        };
        let points = s
            .daily
            .iter()
            .zip(&coords)
            .map(|(d, (x, y))| Point {
                x: *x,
                y: *y,
                label: format!(
                    "{} · {} {}",
                    d.date,
                    if q.metric == "bytes" {
                        bytes(value(d, &q.metric))
                    } else {
                        number(value(d, &q.metric))
                    },
                    label
                ),
            })
            .collect();
        let first = s.daily.first().map(|d| d.date.clone()).unwrap_or_default();
        let last = s.daily.last().map(|d| d.date.clone()).unwrap_or_default();
        let specs = [
            (
                "downloads",
                "package downloads",
                number(s.totals.downloads),
                "complete 200 responses",
                "red",
            ),
            (
                "clients",
                "unique clients",
                number(s.clients),
                "estimated from network addresses",
                "blue",
            ),
            (
                "bytes",
                "bytes served",
                bytes(s.totals.bytes),
                "streamed by this origin",
                "green",
            ),
            (
                "assets",
                "distinct assets",
                number(s.assets),
                "repository files and UI assets",
                "amber",
            ),
        ];
        let metrics = specs
            .into_iter()
            .map(|(key, name, value, note, class)| Metric {
                name,
                value,
                note,
                class,
                line: curve(&coordinates(
                    &s.daily
                        .iter()
                        .map(|d| self::value(d, key))
                        .collect::<Vec<_>>(),
                    220.0,
                    56.0,
                    3.0,
                )),
            })
            .collect();
        let ranks = |rows: Vec<xxc_aptd_core::analytics::Ranked>, downloads: bool| {
            let max = rows
                .iter()
                .map(|r| if downloads { r.downloads } else { r.requests })
                .max()
                .unwrap_or(0)
                .max(1) as f64;
            rows.into_iter()
                .map(|r| Popular {
                    width: 100.0 * (if downloads { r.downloads } else { r.requests }) as f64 / max,
                    name: r.name,
                    downloads: number(r.downloads),
                    requests: number(r.requests),
                    bytes: bytes(r.bytes),
                })
                .collect()
        };
        let mut offset = 0.0;
        let segments = s
            .breakdown
            .into_iter()
            .map(|b| {
                let percent = b.requests as f64 / s.totals.requests.max(1) as f64 * 100.0;
                let segment = Segment {
                    class: match b.kind.as_str() {
                        "packages" => "red",
                        "repository" => "blue",
                        "assets" => "amber",
                        _ => "green",
                    },
                    name: b.kind,
                    requests: number(b.requests),
                    dash: format!("{percent:.3} {:.3}", 100.0 - percent),
                    offset: -offset,
                };
                offset += percent;
                segment
            })
            .collect();
        let rows = s
            .daily
            .iter()
            .map(|d| Row {
                date: d.date.clone(),
                downloads: number(d.counts.downloads),
                requests: number(d.counts.requests),
                clients: number(d.clients),
                bytes: bytes(d.counts.bytes),
            })
            .collect();
        Self {
            enabled,
            days: q.days,
            metric: q.metric,
            label,
            metrics,
            line,
            area,
            points,
            maximum,
            first,
            last,
            packages: ranks(s.packages, true),
            assets: ranks(s.popular_assets, false),
            segments,
            rows,
            total_requests: number(s.totals.requests),
            ranges: number(s.totals.ranges),
            errors: number(s.totals.errors),
            interrupted: number(s.totals.interrupted),
            dropped,
            retention,
            public_origin,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_are_finite_and_flat_series_stay_flat() {
        let points = coordinates(&[0, 0, 0, 0], 1000.0, 220.0, 20.0);
        assert!(points.iter().all(|(_, y)| *y == 200.0));
        let line = curve(&points);
        assert!(!line.contains("NaN") && !line.contains("inf"));
        assert_eq!(number(1234567), "1,234,567");
        assert_eq!(bytes(2048), "2.0 KiB");
        let spike = curve(&coordinates(&[0, 100, 0], 1000.0, 220.0, 20.0));
        assert!(spike.contains("C"));
    }
}
