//! Flotation tank simulation harness.
//!
//! This is intentionally simple: it provides deterministic plant dynamics
//! and named process variables so thesis validation can produce repeatable
//! evidence before physical hardware is available.

use crate::process_image::{PlcValue, ProcessImage, ProcessVariable};

/// Compact flotation-tank process model used for validation.
#[derive(Debug, Clone)]
pub struct FlotationTankSim {
    pub level: f64,
    pub air_flow: f64,
    pub feed_flow: f64,
    pub tailings_flow: f64,
    pub concentrate_grade: f64,
    pub emergency_stop: bool,
    pub motor_running: bool,
}

impl Default for FlotationTankSim {
    fn default() -> Self {
        Self {
            level: 50.0,
            air_flow: 30.0,
            feed_flow: 40.0,
            tailings_flow: 38.0,
            concentrate_grade: 82.0,
            emergency_stop: false,
            motor_running: true,
        }
    }
}

impl FlotationTankSim {
    pub fn seed_process_image(&self) -> ProcessImage {
        let mut image = ProcessImage::new();
        image.insert(
            ProcessVariable::new("tank.level", PlcValue::F64(self.level))
                .with_description("Tank level percentage"),
        );
        image.insert(
            ProcessVariable::new("tank.air_flow", PlcValue::F64(self.air_flow))
                .with_description("Air flow percentage"),
        );
        image.insert(
            ProcessVariable::new("tank.feed_flow", PlcValue::F64(self.feed_flow))
                .with_description("Feed flow percentage"),
        );
        image.insert(
            ProcessVariable::new("tank.tailings_flow", PlcValue::F64(self.tailings_flow))
                .with_description("Tailings flow percentage"),
        );
        image.insert(
            ProcessVariable::new(
                "tank.concentrate_grade",
                PlcValue::F64(self.concentrate_grade),
            )
            .read_only()
            .with_description("Simulated concentrate grade"),
        );
        image.insert(
            ProcessVariable::new("tank.emergency_stop", PlcValue::Bool(self.emergency_stop))
                .with_description("Emergency stop input"),
        );
        image.insert(
            ProcessVariable::new("tank.motor_running", PlcValue::Bool(self.motor_running))
                .with_description("Agitator/motor command"),
        );
        image
    }

    pub fn load_from_image(&mut self, image: &ProcessImage) {
        if let Some(v) = image.get_f64("tank.level") {
            self.level = v;
        }
        if let Some(v) = image.get_f64("tank.air_flow") {
            self.air_flow = v;
        }
        if let Some(v) = image.get_f64("tank.feed_flow") {
            self.feed_flow = v;
        }
        if let Some(v) = image.get_f64("tank.tailings_flow") {
            self.tailings_flow = v;
        }
        if let Some(v) = image.get_bool("tank.emergency_stop") {
            self.emergency_stop = v;
        }
        if let Some(v) = image.get_bool("tank.motor_running") {
            self.motor_running = v;
        }
    }

    /// Advance plant state by one scan period.
    pub fn step(&mut self, dt_seconds: f64) {
        if self.emergency_stop || !self.motor_running {
            self.air_flow *= 0.95;
            self.tailings_flow *= 0.98;
        }

        let net_flow = self.feed_flow - self.tailings_flow;
        self.level = (self.level + net_flow * dt_seconds * 0.05).clamp(0.0, 100.0);

        let air_effect = (self.air_flow - 25.0) * 0.03;
        let level_penalty = (self.level - 55.0).abs() * 0.02;
        self.concentrate_grade = (82.0 + air_effect - level_penalty).clamp(0.0, 100.0);
    }

    pub fn write_to_image(&self, image: &mut ProcessImage) {
        let _ = image.set("tank.level", PlcValue::F64(self.level));
        let _ = image.set("tank.air_flow", PlcValue::F64(self.air_flow));
        let _ = image.set("tank.feed_flow", PlcValue::F64(self.feed_flow));
        let _ = image.set("tank.tailings_flow", PlcValue::F64(self.tailings_flow));
        // read-only variable is intentionally not updated through ProcessImage::set
        if let Some(var) = image.iter().find(|v| v.name == "tank.concentrate_grade") {
            let _ = var;
        }
    }

    pub fn telemetry_csv_header() -> &'static str {
        "cycle,level,air_flow,feed_flow,tailings_flow,concentrate_grade,emergency_stop,motor_running"
    }

    pub fn telemetry_csv_row(&self, cycle: u64) -> String {
        format!(
            "{},{:.3},{:.3},{:.3},{:.3},{:.3},{},{}",
            cycle,
            self.level,
            self.air_flow,
            self.feed_flow,
            self.tailings_flow,
            self.concentrate_grade,
            self.emergency_stop,
            self.motor_running,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_changes_grade() {
        let mut sim = FlotationTankSim::default();
        let before = sim.concentrate_grade;
        sim.air_flow = 60.0;
        sim.step(0.1);
        assert!(sim.concentrate_grade > before);
    }
}
