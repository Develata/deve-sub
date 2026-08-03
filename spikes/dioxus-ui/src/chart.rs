use crate::mock::TrafficPoint;

pub struct ChartConfig {
    pub width: f64,
    pub height: f64,
    pub padding: f64,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self { width: 760.0, height: 280.0, padding: 40.0 }
    }
}

pub struct ChartPaths {
    pub upload_path: String,
    pub download_path: String,
    pub upload_area: String,
    pub download_area: String,
    pub y_max: f64,
    pub points: Vec<(f64, f64, f64)>,
}

pub fn build_chart(data: &[TrafficPoint], cfg: &ChartConfig) -> ChartPaths {
    let n = data.len();
    let inner_w = cfg.width - cfg.padding * 2.0;
    let inner_h = cfg.height - cfg.padding * 2.0;
    let y_max = data
        .iter()
        .map(|d| d.upload_gb.max(d.download_gb))
        .fold(0.0_f64, f64::max)
        .mul_add(1.1, 0.0)
        .max(1.0);

    let x_step = if n > 1 { inner_w / (n - 1) as f64 } else { 0.0 };
    let y_scale = |v: f64| cfg.padding + inner_h - (v / y_max) * inner_h;
    let x_pos = |i: usize| cfg.padding + i as f64 * x_step;

    let points: Vec<(f64, f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, d)| (x_pos(i), y_scale(d.upload_gb), y_scale(d.download_gb)))
        .collect();

    let make_path = |idx: usize| -> String {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = if i == 0 { 'M' } else { 'L' };
                let y = if idx == 0 { p.1 } else { p.2 };
                format!("{cmd} {:.1} {:.1}", p.0, y)
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let make_area = |idx: usize| -> String {
        let path = make_path(idx);
        let base_y = cfg.padding + inner_h;
        let last_x = points.last().map(|p| p.0).unwrap_or(cfg.padding);
        let first_x = points.first().map(|p| p.0).unwrap_or(cfg.padding);
        format!("{path} L {last_x:.1} {base_y:.1} L {first_x:.1} {base_y:.1} Z")
    };

    ChartPaths {
        upload_path: make_path(0),
        download_path: make_path(1),
        upload_area: make_area(0),
        download_area: make_area(1),
        y_max,
        points,
    }
}

pub fn y_axis_labels(y_max: f64, cfg: &ChartConfig, count: usize) -> Vec<(f64, String)> {
    let inner_h = cfg.height - cfg.padding * 2.0;
    (0..=count)
        .map(|i| {
            let ratio = i as f64 / count as f64;
            let value = y_max * (1.0 - ratio);
            let y = cfg.padding + ratio * inner_h;
            (y, format!("{:.0}", value))
        })
        .collect()
}

pub fn x_axis_labels(days: u32, cfg: &ChartConfig, count: usize) -> Vec<(f64, String)> {
    let inner_w = cfg.width - cfg.padding * 2.0;
    (0..=count)
        .map(|i| {
            let ratio = i as f64 / count as f64;
            let x = cfg.padding + ratio * inner_w;
            let day = (ratio * days as f64).round() as u32;
            (x, format!("D{day}"))
        })
        .collect()
}
