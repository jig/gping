use anyhow::Context;
use chrono::prelude::*;
use core::option::Option;
use core::option::Option::{None, Some};
use core::time::Duration;
use itertools::Itertools;
use tui::style::Style;
use tui::symbols;
use tui::widgets::{Dataset, GraphType, Paragraph};

pub struct PlotData {
    pub display: String,
    pub data: Vec<(f64, f64)>,
    pub style: Style,
    buffer: chrono::Duration,
    simple_graphics: bool,
    /// Weight given to the newest sample by the exponential moving average, if smoothing is on.
    smoothing_alpha: Option<f64>,
    /// The smoothed counterpart of `data`, kept in step with it. Empty when smoothing is off.
    smoothed: Vec<(f64, f64)>,
    /// Last smoothed value, carried over timeouts.
    ema: Option<f64>,
}

impl PlotData {
    pub fn new(
        display: String,
        buffer: u64,
        style: Style,
        simple_graphics: bool,
        smoothing_alpha: Option<f64>,
    ) -> PlotData {
        PlotData {
            display,
            data: Vec::with_capacity(150),
            style,
            buffer: chrono::Duration::try_seconds(buffer as i64)
                .with_context(|| format!("Error converting {buffer} to seconds"))
                .unwrap(),
            simple_graphics,
            smoothing_alpha,
            smoothed: Vec::new(),
            ema: None,
        }
    }
    pub fn update(&mut self, item: Option<Duration>) {
        let now = Local::now();
        let idx = now.timestamp_millis() as f64 / 1_000f64;
        let value = match item {
            Some(dur) => dur.as_micros() as f64,
            None => f64::NAN,
        };
        self.data.push((idx, value));
        if let Some(alpha) = self.smoothing_alpha {
            // A timeout carries no round trip time, so the average is left frozen and the gap is
            // kept in the plotted line rather than being bridged over.
            if !value.is_nan() {
                self.ema = Some(match self.ema {
                    Some(previous) => alpha * value + (1f64 - alpha) * previous,
                    None => value,
                });
            }
            self.smoothed.push((
                idx,
                if value.is_nan() {
                    f64::NAN
                } else {
                    self.ema.unwrap()
                },
            ));
        }
        // Find the last index that we should remove.
        let earliest_timestamp = (now - self.buffer).timestamp_millis() as f64 / 1_000f64;
        let last_idx = self
            .data
            .iter()
            .enumerate()
            .filter(|(_, (timestamp, _))| *timestamp < earliest_timestamp)
            .map(|(idx, _)| idx)
            .next_back();
        if let Some(idx) = last_idx {
            self.data.drain(0..idx).for_each(drop);
            if !self.smoothed.is_empty() {
                self.smoothed.drain(0..idx).for_each(drop);
            }
        }
    }

    /// The points that end up on the graph: the smoothed series when smoothing is on, the raw
    /// samples otherwise. The header stats always use the raw samples.
    pub fn plot_points(&self) -> &[(f64, f64)] {
        if self.smoothing_alpha.is_some() {
            self.smoothed.as_slice()
        } else {
            self.data.as_slice()
        }
    }

    /// The plotted points with their values pulled inside `bounds`. The chart widget simply drops
    /// anything outside of the axis bounds, so clamping keeps out-of-range replies visible as a
    /// line running along the top or the bottom of the graph. Timeouts stay `NaN`.
    pub fn clamped_data(&self, bounds: [f64; 2]) -> Vec<(f64, f64)> {
        let [lower, upper] = bounds;
        self.plot_points()
            .iter()
            .map(|&(timestamp, value)| (timestamp, value.clamp(lower, upper)))
            .collect()
    }

    pub fn header_stats(&self) -> Vec<Paragraph<'_>> {
        let ping_header = Paragraph::new(self.display.clone()).style(self.style);
        let items: Vec<&f64> = self
            .data
            .iter()
            .filter(|(_, x)| !x.is_nan())
            .map(|(_, v)| v)
            .sorted_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .collect();
        if items.is_empty() {
            return vec![ping_header];
        }

        let min = **items.first().unwrap();
        let max = **items.last().unwrap();
        let avg = items.iter().copied().sum::<f64>() / items.len() as f64;
        let jtr = items
            .iter()
            .zip(items.iter().skip(1))
            .map(|(&prev, &curr)| (curr - prev).abs())
            .sum::<f64>()
            / (items.len() - 1) as f64;

        let percentile_position = 0.95 * items.len() as f32;
        let rounded_position = percentile_position.round() as usize;
        let p95 = items.get(rounded_position).map(|i| **i).unwrap_or(0f64);

        // count timeouts
        let to = self.data.iter().filter(|(_, x)| x.is_nan()).count();

        let last = self.data.last().unwrap_or(&(0f64, 0f64)).1;

        vec![
            ping_header,
            Paragraph::new(format!("last {:?}", Duration::from_micros(last as u64)))
                .style(self.style),
            Paragraph::new(format!("min {:?}", Duration::from_micros(min as u64)))
                .style(self.style),
            Paragraph::new(format!("max {:?}", Duration::from_micros(max as u64)))
                .style(self.style),
            Paragraph::new(format!("avg {:?}", Duration::from_micros(avg as u64)))
                .style(self.style),
            Paragraph::new(format!("jtr {:?}", Duration::from_micros(jtr as u64)))
                .style(self.style),
            Paragraph::new(format!("p95 {:?}", Duration::from_micros(p95 as u64)))
                .style(self.style),
            Paragraph::new(format!("t/o {to:?}")).style(self.style),
        ]
    }
}

impl<'a> From<&'a PlotData> for Dataset<'a> {
    fn from(plot: &'a PlotData) -> Self {
        let slice = plot.plot_points();
        Dataset::default()
            .marker(if plot.simple_graphics {
                symbols::Marker::Dot
            } else {
                symbols::Marker::Braille
            })
            .style(plot.style)
            .graph_type(GraphType::Line)
            .data(slice)
    }
}
