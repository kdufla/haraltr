pub struct KalmanFilter {
    x: f64, // Estimate
    p: f64, // Covariance of the estimate
    q: f64, // Covariance of the process noise
    r: f64, // Covariance of the observation noise
}

impl KalmanFilter {
    pub fn new(initial_value: f64, q: f64, r: f64) -> Self {
        Self {
            x: initial_value,
            p: 1.0,
            q,
            r,
        }
    }

    pub fn update(&mut self, z: f64) -> f64 {
        self.p += self.q;
        let k = self.p / (self.p + self.r);
        self.x += k * (z - self.x);
        self.p *= 1.0 - k;
        self.x
    }

    pub fn _update_params(&mut self, q: f64, r: f64) {
        self.q = q;
        self.r = r;
    }
}
