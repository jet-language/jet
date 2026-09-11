// D-DX-SUITE1=C / #2475: headless viewport virtualization.
//
// A virtual window is a compact layout fact.  It never allocates one entry per
// source row: even a 100,000-row table produces only the bounded visible range.
// Renderers consume the range and choose their own DOM/TUI representation.

const JET_WEB_VIRTUAL_MAX_MEASUREMENTS: usize = 4_096;
const JET_WEB_VIRTUAL_MAX_VISIBLE: i64 = 4_096;
#[derive(Clone)]
pub struct JetWebVirtualViewport {
    window: jet_std::JetSignal<JetWebVirtualWindow>,
}
#[derive(Clone, Copy)]
pub struct JetWebVirtualWindow {
    pub total_count: i64,
    pub scroll_offset: i64,
    pub viewport_size: i64,
    pub estimated_item_size: i64,
    pub overscan: i64,
    /// Inclusive start and exclusive end row indices.
    pub start: i64,
    pub end: i64,
    /// Estimated full content extent in the same units as `viewport_size`.
    pub total_size: i64,
    /// Number of positive measurements used for the estimate.
    pub measured_count: i64,
}

impl JetWebVirtualWindow {
    pub fn visible_count(&self) -> i64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn contains(&self, index: i64) -> bool {
        index >= self.start && index < self.end
    }

    pub fn range(&self) -> (i64, i64) {
        (self.start, self.end)
    }

    /// Recompute the estimate after one measured row.  The window remains a
    /// value, so a renderer can replace it atomically after measuring a frame.
    pub fn with_measurement(&self, index: i64, size: i64) -> Self {
        if index < 0 || index >= self.total_count || size <= 0 {
            return *self;
        }
        let measured_count = self.measured_count.saturating_add(1);
        let measured_total = self
            .estimated_item_size
            .saturating_mul(self.measured_count)
            .saturating_add(size);
        let estimate = measured_total
            .checked_div(measured_count)
            .unwrap_or(self.estimated_item_size)
            .max(1);
        jet_web_virtual_window(
            self.total_count,
            self.scroll_offset,
            self.viewport_size,
            estimate,
            self.overscan,
        )
        .with_measured_count(measured_count)
    }

    fn with_measured_count(mut self, measured_count: i64) -> Self {
        self.measured_count = measured_count.max(0);
        self
    }

    pub fn within_frame_budget(&self, max_visible_items: i64) -> bool {
        self.visible_count() <= max_visible_items.max(0)
    }

    pub fn facts_json(&self) -> String {
        format!(
            "{{\"total_count\":{},\"scroll_offset\":{},\"viewport_size\":{},\"estimated_item_size\":{},\"overscan\":{},\"start\":{},\"end\":{},\"total_size\":{},\"measured_count\":{}}}",
            self.total_count,
            self.scroll_offset,
            self.viewport_size,
            self.estimated_item_size,
            self.overscan,
            self.start,
            self.end,
            self.total_size,
            self.measured_count,
        )
    }
}

/// Build one bounded virtual range.  `overscan` is measured in rows; size and
/// offsets are in the renderer's units (normally CSS pixels or terminal rows).
pub fn jet_web_virtual_window(
    total_count: i64,
    scroll_offset: i64,
    viewport_size: i64,
    estimated_item_size: i64,
    overscan: i64,
) -> JetWebVirtualWindow {
    let total_count = total_count.max(0);
    let viewport_size = viewport_size.max(0);
    let estimated_item_size = estimated_item_size.max(1);
    let overscan = overscan.clamp(0, JET_WEB_VIRTUAL_MAX_VISIBLE);
    let total_size = total_count.saturating_mul(estimated_item_size);
    let max_offset = total_size.saturating_sub(viewport_size);
    let scroll_offset = scroll_offset.max(0).min(max_offset);
    let first_visible = if total_count == 0 {
        0
    } else {
        (scroll_offset / estimated_item_size).min(total_count)
    };
    let visible_rows = if viewport_size == 0 {
        0
    } else {
        viewport_size
            .saturating_add(estimated_item_size.saturating_sub(1))
            .checked_div(estimated_item_size)
            .unwrap_or(0)
            .min(JET_WEB_VIRTUAL_MAX_VISIBLE)
    };
    let last_visible = first_visible
        .saturating_add(visible_rows)
        .min(total_count);
    let requested_start = first_visible.saturating_sub(overscan);
    let requested_end = last_visible.saturating_add(overscan).min(total_count);
    let (start, end) =
        if requested_end.saturating_sub(requested_start) > JET_WEB_VIRTUAL_MAX_VISIBLE {
            let start = first_visible
                .saturating_sub(JET_WEB_VIRTUAL_MAX_VISIBLE / 2)
                .min(total_count);
            (
                start,
                start
                    .saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE)
                    .min(total_count),
            )
        } else {
            (requested_start, requested_end)
        };
    JetWebVirtualWindow {
        total_count,
        scroll_offset,
        viewport_size,
        estimated_item_size,
        overscan,
        start,
        end,
        total_size,
        measured_count: 0,
    }
}

impl JetWebVirtualViewport {
    pub fn new(window: JetWebVirtualWindow) -> Self {
        Self {
            window: jet_std::JetSignal::new(window),
        }
    }

    pub fn window(&self) -> JetWebVirtualWindow {
        self.window.get()
    }

    pub fn signal(&self) -> jet_std::JetSignal<JetWebVirtualWindow> {
        self.window.clone()
    }

    pub fn set_window(&self, window: JetWebVirtualWindow) {
        self.window.set(window);
    }

    pub fn scroll_to(&self, scroll_offset: i64) {
        let current = self.window();
        self.set_window(
            jet_web_virtual_window(
                current.total_count,
                scroll_offset,
                current.viewport_size,
                current.estimated_item_size,
                current.overscan,
            )
            .with_measured_count(current.measured_count),
        );
    }

    pub fn resize(&self, viewport_size: i64) {
        let current = self.window();
        self.set_window(
            jet_web_virtual_window(
                current.total_count,
                current.scroll_offset,
                viewport_size,
                current.estimated_item_size,
                current.overscan,
            )
            .with_measured_count(current.measured_count),
        );
    }

    pub fn measure(&self, index: i64, size: i64) {
        let current = self.window();
        self.set_window(current.with_measurement(index, size));
    }

    pub fn facts_json(&self) -> String {
        self.window().facts_json()
    }
}

/// Build a window with a dynamic estimate from positive measured row sizes.
/// Invalid measurements are ignored.  The source vector can stay sparse; no
/// per-row layout allocation is required.
pub fn jet_web_virtual_window_measured(
    total_count: i64,
    scroll_offset: i64,
    viewport_size: i64,
    fallback_item_size: i64,
    overscan: i64,
    measurements: &Vec<i64>,
) -> JetWebVirtualWindow {
    let (measured_total, measured_count) = measurements
        .iter()
        .copied()
        .filter(|size| *size > 0)
        .take(JET_WEB_VIRTUAL_MAX_MEASUREMENTS)
        .fold((0_i64, 0_i64), |(total, count), size| {
            (total.saturating_add(size), count.saturating_add(1))
        });
    let estimate = if measured_count == 0 {
        fallback_item_size.max(1)
    } else {
        measured_total
            .checked_div(measured_count)
            .unwrap_or(fallback_item_size.max(1))
            .max(1)
    };
    let mut window = jet_web_virtual_window(
        total_count,
        scroll_offset,
        viewport_size,
        estimate,
        overscan,
    );
    window.measured_count = measured_count;
    window
}

/// Copy only rows in the virtual range.  The returned length is bounded by the
/// window, not by the source list, and malformed/stale ranges clamp safely.
pub fn jet_web_virtual_slice<T: Clone>(
    rows: &Vec<T>,
    window: &JetWebVirtualWindow,
) -> Vec<T> {
    let start = window.start.max(0).try_into().unwrap_or(usize::MAX).min(rows.len());
    let end = window
        .end
        .max(window.start)
        .try_into()
        .unwrap_or(usize::MAX)
        .min(rows.len())
        .min(start.saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE as usize));
    if start >= end {
        Vec::new()
    } else {
        rows[start..end].to_vec()
    }
}

pub fn jet_web_virtual_indices(window: &JetWebVirtualWindow) -> Vec<i64> {
    if window.start >= window.end {
        return Vec::new();
    }
    let end = window
        .end
        .min(window.start.saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE));
    (window.start..end).collect()
}

pub fn jet_web_virtual_viewport(window: JetWebVirtualWindow) -> JetWebVirtualViewport {
    JetWebVirtualViewport::new(window)
}

/// A bounded, renderer-independent virtualization plan. Measurements are
/// sparse `(row_index, size)` pairs, so a 100,000-row source never allocates a
/// 100,000-entry layout table.
#[derive(Clone, Debug, PartialEq)]
pub struct JetWebVirtualPlan {
    pub total_count: i64,
    pub scroll_offset: i64,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub estimated_item_size: i64,
    pub overscan: i64,
    /// Inclusive start and exclusive end row indices.
    pub start: i64,
    pub end: i64,
    pub total_size: i64,
    pub measured_count: i64,
    /// The first visible row and its offset from the viewport origin. Keeping
    /// this pair makes a measurement update preserve the user's anchor.
    pub anchor_index: i64,
    pub anchor_offset: i64,
    pub measurements: Vec<(i64, i64)>,
}

impl JetWebVirtualPlan {
    pub fn visible_count(&self) -> i64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn contains(&self, index: i64) -> bool {
        index >= self.start && index < self.end
    }

    pub fn range(&self) -> (i64, i64) {
        (self.start, self.end)
    }

    pub fn row_size(&self, index: i64) -> i64 {
        self.measurements
            .binary_search_by_key(&index, |(row, _)| *row)
            .ok()
            .and_then(|position| self.measurements.get(position).map(|(_, size)| *size))
            .unwrap_or(self.estimated_item_size)
    }

    pub fn offset_for(&self, index: i64) -> i64 {
        jet_web_virtual_plan_offset(
            self.total_count,
            self.estimated_item_size,
            &self.measurements,
            index,
        )
    }

    pub fn indices(&self) -> Vec<i64> {
        jet_web_virtual_plan_indices(self)
    }

    /// Accept a bounded measurement exchange from a renderer. Invalid or
    /// out-of-range measurements are ignored, duplicate indices are updated in
    /// place, and the old anchor remains at the same viewport offset.
    pub fn measurement_exchange(&self, updates: &[(i64, i64)]) -> Self {
        let mut measurements = self.measurements.clone();
        for &(index, size) in updates.iter().take(JET_WEB_VIRTUAL_MAX_MEASUREMENTS) {
            jet_web_virtual_upsert_measurement(
                &mut measurements,
                self.total_count,
                index,
                size,
            );
        }
        let estimated_item_size =
            jet_web_virtual_plan_estimate(self.estimated_item_size, &measurements);
        jet_web_virtual_plan_reflow(
            self.total_count,
            self.scroll_offset,
            self.viewport_width,
            self.viewport_height,
            estimated_item_size,
            self.overscan,
            measurements,
            Some((self.anchor_index, self.anchor_offset)),
        )
    }

    pub fn with_measurement(&self, index: i64, size: i64) -> Self {
        self.measurement_exchange(&[(index, size)])
    }

    pub fn with_measurements(&self, measurements: &[(i64, i64)]) -> Self {
        self.measurement_exchange(measurements)
    }

    pub fn within_frame_budget(&self, max_visible_items: i64) -> bool {
        self.visible_count() <= max_visible_items.max(0)
    }

    pub fn facts_json(&self) -> String {
        let measurements = self
            .measurements
            .iter()
            .map(|(index, size)| format!("{{\"index\":{index},\"size\":{size}}}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"total_count\":{},\"scroll_offset\":{},\"viewport_width\":{},\"viewport_height\":{},\"estimated_item_size\":{},\"overscan\":{},\"start\":{},\"end\":{},\"total_size\":{},\"measured_count\":{},\"anchor_index\":{},\"anchor_offset\":{},\"measurements\":[{}]}}",
            self.total_count,
            self.scroll_offset,
            self.viewport_width,
            self.viewport_height,
            self.estimated_item_size,
            self.overscan,
            self.start,
            self.end,
            self.total_size,
            self.measured_count,
            self.anchor_index,
            self.anchor_offset,
            measurements,
        )
    }
}

#[derive(Clone)]
pub struct JetWebVirtualPlanViewport {
    plan: jet_std::JetSignal<JetWebVirtualPlan>,
}

impl JetWebVirtualPlanViewport {
    pub fn new(plan: JetWebVirtualPlan) -> Self {
        Self {
            plan: jet_std::JetSignal::new(plan),
        }
    }

    pub fn plan(&self) -> JetWebVirtualPlan {
        self.plan.get()
    }

    pub fn signal(&self) -> jet_std::JetSignal<JetWebVirtualPlan> {
        self.plan.clone()
    }

    pub fn set_plan(&self, plan: JetWebVirtualPlan) {
        self.plan.set(plan);
    }

    pub fn scroll_to(&self, scroll_offset: i64) {
        let plan = self.plan();
        self.set_plan(jet_web_virtual_plan_reflow(
            plan.total_count,
            scroll_offset,
            plan.viewport_width,
            plan.viewport_height,
            plan.estimated_item_size,
            plan.overscan,
            plan.measurements,
            None,
        ));
    }

    pub fn resize(&self, viewport_width: i64, viewport_height: i64) {
        let plan = self.plan();
        self.set_plan(jet_web_virtual_plan_reflow(
            plan.total_count,
            plan.scroll_offset,
            viewport_width,
            viewport_height,
            plan.estimated_item_size,
            plan.overscan,
            plan.measurements,
            Some((plan.anchor_index, plan.anchor_offset)),
        ));
    }

    pub fn measure(&self, index: i64, size: i64) {
        let plan = self.plan();
        self.set_plan(plan.with_measurement(index, size));
    }

    pub fn measure_exchange(&self, measurements: &[(i64, i64)]) {
        let plan = self.plan();
        self.set_plan(plan.with_measurements(measurements));
    }

    pub fn facts_json(&self) -> String {
        self.plan().facts_json()
    }
}

pub fn jet_web_virtual_plan(
    total_count: i64,
    scroll_offset: i64,
    viewport_width: i64,
    viewport_height: i64,
    estimated_item_size: i64,
    overscan: i64,
) -> JetWebVirtualPlan {
    jet_web_virtual_plan_reflow(
        total_count,
        scroll_offset,
        viewport_width,
        viewport_height,
        estimated_item_size,
        overscan,
        Vec::new(),
        None,
    )
}

pub fn jet_web_virtual_plan_measured(
    total_count: i64,
    scroll_offset: i64,
    viewport_width: i64,
    viewport_height: i64,
    fallback_item_size: i64,
    overscan: i64,
    measurements: &Vec<(i64, i64)>,
) -> JetWebVirtualPlan {
    let mut measurements = measurements
        .iter()
        .copied()
        .filter(|(index, size)| *index >= 0 && *index < total_count.max(0) && *size > 0)
        .take(JET_WEB_VIRTUAL_MAX_MEASUREMENTS)
        .collect::<Vec<_>>();
    jet_web_virtual_normalize_measurements(total_count, &mut measurements);
    let estimate = jet_web_virtual_plan_estimate(fallback_item_size, &measurements);
    jet_web_virtual_plan_reflow(
        total_count,
        scroll_offset,
        viewport_width,
        viewport_height,
        estimate,
        overscan,
        measurements,
        None,
    )
}

/// Convenience form for a renderer that reports sizes for consecutive rows.
pub fn jet_web_virtual_plan_from_sizes(
    total_count: i64,
    scroll_offset: i64,
    viewport_width: i64,
    viewport_height: i64,
    fallback_item_size: i64,
    overscan: i64,
    sizes: &Vec<i64>,
) -> JetWebVirtualPlan {
    let measurements = sizes
        .iter()
        .enumerate()
        .filter_map(|(index, size)| {
            (*size > 0).then_some((i64::try_from(index).unwrap_or(i64::MAX), *size))
        })
        .take(JET_WEB_VIRTUAL_MAX_MEASUREMENTS)
        .collect::<Vec<_>>();
    jet_web_virtual_plan_measured(
        total_count,
        scroll_offset,
        viewport_width,
        viewport_height,
        fallback_item_size,
        overscan,
        &measurements,
    )
}

pub fn jet_web_virtual_plan_indices(plan: &JetWebVirtualPlan) -> Vec<i64> {
    if plan.start >= plan.end {
        return Vec::new();
    }
    let end = plan
        .end
        .min(plan.start.saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE as i64));
    (plan.start..end).collect()
}

pub fn jet_web_virtual_plan_slice<T: Clone>(
    rows: &Vec<T>,
    plan: &JetWebVirtualPlan,
) -> Vec<T> {
    let start = plan.start.max(0).try_into().unwrap_or(usize::MAX).min(rows.len());
    let end = plan
        .end
        .max(plan.start)
        .try_into()
        .unwrap_or(usize::MAX)
        .min(rows.len())
        .min(start.saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE as usize));
    if start >= end {
        Vec::new()
    } else {
        rows[start..end].to_vec()
    }
}

fn jet_web_virtual_normalize_measurements(
    total_count: i64,
    measurements: &mut Vec<(i64, i64)>,
) {
    let total_count = total_count.max(0);
    measurements.retain(|(index, size)| {
        *index >= 0 && *index < total_count && *size > 0
    });
    measurements.sort_by_key(|(index, _)| *index);
    // Keep the newest value when an exchange contains the same row more than
    // once. The reverse/dedup/reverse sequence preserves that value while
    // restoring ascending order for binary-search offset calculations.
    measurements.reverse();
    measurements.dedup_by_key(|(index, _)| *index);
    measurements.reverse();
    measurements.truncate(JET_WEB_VIRTUAL_MAX_MEASUREMENTS);
}

fn jet_web_virtual_plan_estimate(
    fallback_item_size: i64,
    measurements: &[(i64, i64)],
) -> i64 {
    let (total, count) = measurements
        .iter()
        .take(JET_WEB_VIRTUAL_MAX_MEASUREMENTS)
        .filter(|(_, size)| *size > 0)
        .fold((0_i64, 0_i64), |(total, count), (_, size)| {
            (total.saturating_add(*size), count.saturating_add(1))
        });
    if count == 0 {
        fallback_item_size.max(1)
    } else {
        total
            .checked_div(count)
            .unwrap_or(fallback_item_size.max(1))
            .max(1)
    }
}

fn jet_web_virtual_plan_reflow(
    total_count: i64,
    scroll_offset: i64,
    viewport_width: i64,
    viewport_height: i64,
    estimated_item_size: i64,
    overscan: i64,
    measurements: Vec<(i64, i64)>,
    anchor: Option<(i64, i64)>,
) -> JetWebVirtualPlan {
    let total_count = total_count.max(0);
    let viewport_width = viewport_width.max(0);
    let viewport_height = viewport_height.max(0);
    let estimated_item_size = estimated_item_size.max(1);
    let overscan = overscan.clamp(0, JET_WEB_VIRTUAL_MAX_VISIBLE);
    let mut measurements = measurements;
    jet_web_virtual_normalize_measurements(total_count, &mut measurements);

    let total_size = jet_web_virtual_plan_total_size(
        total_count,
        estimated_item_size,
        &measurements,
    );
    let max_offset = total_size.saturating_sub(viewport_height);
    let requested_offset = if let Some((anchor_index, anchor_offset)) = anchor {
        jet_web_virtual_plan_offset(
            total_count,
            estimated_item_size,
            &measurements,
            anchor_index,
        )
        .saturating_add(anchor_offset.max(0))
    } else {
        scroll_offset
    };
    let scroll_offset = requested_offset.max(0).min(max_offset);
    let (first_visible, last_visible) = jet_web_virtual_plan_visible_range(
        total_count,
        viewport_height,
        scroll_offset,
        estimated_item_size,
        &measurements,
    );
    let requested_start = first_visible.saturating_sub(overscan);
    let requested_end = last_visible.saturating_add(overscan).min(total_count);
    let (start, end) = if requested_end.saturating_sub(requested_start)
        > JET_WEB_VIRTUAL_MAX_VISIBLE
    {
        let start = first_visible
            .saturating_sub(JET_WEB_VIRTUAL_MAX_VISIBLE / 2)
            .min(total_count);
        (
            start,
            start
                .saturating_add(JET_WEB_VIRTUAL_MAX_VISIBLE)
                .min(total_count),
        )
    } else {
        (requested_start, requested_end)
    };
    let anchor_index = first_visible.min(total_count);
    let anchor_offset = scroll_offset
        .saturating_sub(jet_web_virtual_plan_offset(
            total_count,
            estimated_item_size,
            &measurements,
            anchor_index,
        ));
    JetWebVirtualPlan {
        total_count,
        scroll_offset,
        viewport_width,
        viewport_height,
        estimated_item_size,
        overscan,
        start,
        end,
        total_size,
        measured_count: i64::try_from(measurements.len()).unwrap_or(i64::MAX),
        anchor_index,
        anchor_offset,
        measurements,
    }
}

fn jet_web_virtual_plan_total_size(
    total_count: i64,
    estimated_item_size: i64,
    measurements: &[(i64, i64)],
) -> i64 {
    let mut total = total_count.saturating_mul(estimated_item_size);
    for (_, size) in measurements {
        if *size >= estimated_item_size {
            total = total.saturating_add(size.saturating_sub(estimated_item_size));
        } else {
            total = total.saturating_sub(estimated_item_size.saturating_sub(*size));
        }
    }
    total
}

fn jet_web_virtual_plan_offset(
    total_count: i64,
    estimated_item_size: i64,
    measurements: &[(i64, i64)],
    index: i64,
) -> i64 {
    let index = index.max(0).min(total_count);
    let mut offset = index.saturating_mul(estimated_item_size);
    for (row, size) in measurements {
        if *row >= index {
            break;
        }
        if *size >= estimated_item_size {
            offset = offset.saturating_add(size.saturating_sub(estimated_item_size));
        } else {
            offset = offset.saturating_sub(estimated_item_size.saturating_sub(*size));
        }
    }
    offset
}

fn jet_web_virtual_plan_visible_range(
    total_count: i64,
    viewport_height: i64,
    scroll_offset: i64,
    estimated_item_size: i64,
    measurements: &[(i64, i64)],
) -> (i64, i64) {
    if total_count == 0 || viewport_height == 0 {
        return (0, 0);
    }
    let first_visible = jet_web_virtual_plan_first_row(
        total_count,
        scroll_offset,
        estimated_item_size,
        measurements,
    );
    let viewport_end = scroll_offset.saturating_add(viewport_height);
    let last_visible = jet_web_virtual_plan_first_start_at_or_after(
        total_count,
        viewport_end,
        estimated_item_size,
        measurements,
    )
    .max(first_visible.saturating_add(1))
    .min(total_count);
    (first_visible, last_visible)
}

fn jet_web_virtual_plan_first_row(
    total_count: i64,
    offset: i64,
    estimated_item_size: i64,
    measurements: &[(i64, i64)],
) -> i64 {
    let mut low = 0_i64;
    let mut high = total_count;
    while low < high {
        let middle = low.saturating_add(high).checked_div(2).unwrap_or(low);
        let end = jet_web_virtual_plan_offset(
            total_count,
            estimated_item_size,
            measurements,
            middle.saturating_add(1),
        );
        if end <= offset {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    low.min(total_count)
}

fn jet_web_virtual_plan_first_start_at_or_after(
    total_count: i64,
    offset: i64,
    estimated_item_size: i64,
    measurements: &[(i64, i64)],
) -> i64 {
    let mut low = 0_i64;
    let mut high = total_count;
    while low < high {
        let middle = low.saturating_add(high).checked_div(2).unwrap_or(low);
        let start = jet_web_virtual_plan_offset(
            total_count,
            estimated_item_size,
            measurements,
            middle,
        );
        if start < offset {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    low.min(total_count)
}

fn jet_web_virtual_upsert_measurement(
    measurements: &mut Vec<(i64, i64)>,
    total_count: i64,
    index: i64,
    size: i64,
) {
    if index < 0 || index >= total_count || size <= 0 {
        return;
    }
    if let Some((_, existing)) = measurements
        .iter_mut()
        .find(|(row, _)| *row == index)
    {
        *existing = size;
        return;
    }
    if measurements.len() < JET_WEB_VIRTUAL_MAX_MEASUREMENTS {
        measurements.push((index, size));
    }
}
