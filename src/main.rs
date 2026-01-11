use eframe::egui;
use chrono::{NaiveDate, Duration};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
struct LogEntry {
    timestamp: String,
    message: String,
    level: LogLevel,
}

#[derive(Clone, Copy, PartialEq)]
enum LogLevel {
    Info,
    Success,
    Error,
    Warning,
}

struct VehicleChecker {
    vehicle_no: String,
    start_date: String,
    end_date: String,
    num_threads: usize,

    is_running: Arc<AtomicBool>,
    record_found: Arc<AtomicBool>,
    logs: Arc<Mutex<Vec<LogEntry>>>,
    found_count: Arc<Mutex<usize>>,
    checked_dates: Arc<Mutex<usize>>,
    total_dates: Arc<Mutex<usize>>,

    status_text: String,
    results_dir: PathBuf,
}

impl Default for VehicleChecker {
    fn default() -> Self {
        let results_dir = PathBuf::from("vehicle_results");
        if !results_dir.exists() {
            let _ = fs::create_dir(&results_dir);
        }

        Self {
            vehicle_no: String::new(),
            start_date: "2000-01-01".to_string(),
            end_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            num_threads: 6,
            is_running: Arc::new(AtomicBool::new(false)),
            record_found: Arc::new(AtomicBool::new(false)),
            logs: Arc::new(Mutex::new(Vec::new())),
            found_count: Arc::new(Mutex::new(0)),
            checked_dates: Arc::new(Mutex::new(0)),
            total_dates: Arc::new(Mutex::new(0)),
            status_text: "Ready".to_string(),
            results_dir,
        }
    }
}

impl VehicleChecker {
    fn log(&self, message: String, level: LogLevel) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = LogEntry {
            timestamp,
            message,
            level,
        };

        if let Ok(mut logs) = self.logs.lock() {
            logs.push(entry);
            // Keep only last 1000 entries to prevent memory issues
            if logs.len() > 1000 {
                logs.drain(0..100);
            }
        }
    }

    fn clear_logs(&self) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.clear();
        }
    }

    fn start_checking(&mut self) {
        let vehicle_no = self.vehicle_no.trim().to_uppercase();
        let start_date_str = self.start_date.trim();
        let end_date_str = self.end_date.trim();

        // Validate inputs
        if vehicle_no.is_empty() {
            self.log("Please enter a vehicle registration number".to_string(), LogLevel::Error);
            return;
        }

        let start_date = match NaiveDate::parse_from_str(start_date_str, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                self.log("Invalid start date format. Use YYYY-MM-DD".to_string(), LogLevel::Error);
                return;
            }
        };

        let end_date = match NaiveDate::parse_from_str(end_date_str, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                self.log("Invalid end date format. Use YYYY-MM-DD".to_string(), LogLevel::Error);
                return;
            }
        };

        if start_date > end_date {
            self.log("Starting date must be before ending date".to_string(), LogLevel::Error);
            return;
        }

        let total_days = (end_date - start_date).num_days() + 1;
        let days_per_thread = total_days / self.num_threads as i64;
        let remainder_days = total_days % self.num_threads as i64;

        // Reset state
        self.is_running.store(true, Ordering::SeqCst);
        self.record_found.store(false, Ordering::SeqCst);
        *self.found_count.lock().unwrap() = 0;
        *self.checked_dates.lock().unwrap() = 0;
        *self.total_dates.lock().unwrap() = total_days as usize;

        self.log(format!("Starting check for vehicle: {}", vehicle_no), LogLevel::Info);
        self.log(format!("Date range: {} to {}", start_date_str, end_date_str), LogLevel::Info);
        self.log(format!("Total days to check: {}", total_days), LogLevel::Info);
        self.log(format!("Threads: {}, ~{} days per thread", self.num_threads, days_per_thread), LogLevel::Info);
        self.log(format!("Results will be saved to: {:?}", self.results_dir), LogLevel::Info);
        self.log("Program will STOP automatically when a record is found!".to_string(), LogLevel::Warning);
        self.log("-".repeat(80), LogLevel::Info);

        let results_dir = self.results_dir.clone();
        let logs = Arc::clone(&self.logs);
        let is_running = Arc::clone(&self.is_running);
        let record_found = Arc::clone(&self.record_found);
        let found_count = Arc::clone(&self.found_count);
        let checked_dates = Arc::clone(&self.checked_dates);
        let num_threads = self.num_threads;

        // Spawn threads
        thread::spawn(move || {
            let mut handles = vec![];
            let mut current_start = start_date;

            for i in 0..num_threads {
                let thread_days = days_per_thread + if i < remainder_days as usize { 1 } else { 0 };
                let thread_end = current_start + Duration::days(thread_days - 1);
                let thread_end = if thread_end > end_date { end_date } else { thread_end };

                let log_msg = format!("Thread {}: {} to {}", i + 1,
                                      current_start.format("%Y-%m-%d"), thread_end.format("%Y-%m-%d"));
                Self::log_static(&logs, log_msg, LogLevel::Info);

                let vehicle = vehicle_no.clone();
                let logs_clone = Arc::clone(&logs);
                let is_running_clone = Arc::clone(&is_running);
                let record_found_clone = Arc::clone(&record_found);
                let found_count_clone = Arc::clone(&found_count);
                let checked_dates_clone = Arc::clone(&checked_dates);
                let results_dir_clone = results_dir.clone();
                let thread_id = i + 1;

                let handle = thread::spawn(move || {
                    Self::check_vehicle_thread(
                        vehicle,
                        current_start,
                        thread_end,
                        thread_id,
                        logs_clone,
                        is_running_clone,
                        record_found_clone,
                        found_count_clone,
                        checked_dates_clone,
                        results_dir_clone,
                    );
                });

                handles.push(handle);
                current_start = thread_end + Duration::days(1);

                if current_start > end_date {
                    break;
                }
            }

            Self::log_static(&logs, "-".repeat(80), LogLevel::Info);

            // Wait for all threads
            for handle in handles {
                let _ = handle.join();
            }

            is_running.store(false, Ordering::SeqCst);
        });
    }

    fn log_static(logs: &Arc<Mutex<Vec<LogEntry>>>, message: String, level: LogLevel) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = LogEntry {
            timestamp,
            message,
            level,
        };

        if let Ok(mut logs) = logs.lock() {
            logs.push(entry);
            if logs.len() > 1000 {
                logs.drain(0..100);
            }
        }
    }

    fn check_vehicle_thread(
        vehicle_no: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        thread_id: usize,
        logs: Arc<Mutex<Vec<LogEntry>>>,
        is_running: Arc<AtomicBool>,
        record_found: Arc<AtomicBool>,
        found_count: Arc<Mutex<usize>>,
        checked_dates: Arc<Mutex<usize>>,
        results_dir: PathBuf,
    ) {
        let mut current_date = start_date;
        let mut checked_count = 0;

        while current_date <= end_date && is_running.load(Ordering::SeqCst) && !record_found.load(Ordering::SeqCst) {
            let date_str = current_date.format("%Y-%m-%d").to_string();

            match Self::make_request(&vehicle_no, &date_str) {
                Ok((status, response)) => {
                    checked_count += 1;

                    // Increment global checked dates counter
                    if let Ok(mut count) = checked_dates.lock() {
                        *count += 1;
                    }

                    if status != 200 {
                        let msg = format!("Thread {}: HTTP {} Error - Vehicle: {}, Date: {}",
                                          thread_id, status, vehicle_no, date_str);
                        Self::log_static(&logs, msg, LogLevel::Error);

                        Self::save_response(&vehicle_no, &date_str, &response, thread_id, status, &results_dir, &logs, &found_count);

                        let preview = response.chars().take(300).collect::<String>()
                        .replace('\n', " ").replace('\t', " ");
                        Self::log_static(&logs, format!("Response preview: {}...", preview), LogLevel::Error);

                        record_found.store(true, Ordering::SeqCst);
                        Self::log_static(&logs, format!("Thread {}: Stopping all threads due to HTTP {} error", thread_id, status), LogLevel::Warning);
                        break;
                    } else if response.to_uppercase().contains("NO RECORD FOUND") &&
                        response.to_uppercase().contains("PLEASE CONTACT EXCISE") {
                            if checked_count % 10 == 0 {
                                let msg = format!("Thread {}: Checked {} dates, currently at {} - No records",
                                                  thread_id, checked_count, date_str);
                                Self::log_static(&logs, msg, LogLevel::Info);
                            }
                        } else {
                            let msg = format!("Thread {}: *** RECORD FOUND *** - Vehicle: {}, Date: {}",
                                              thread_id, vehicle_no, date_str);
                            Self::log_static(&logs, msg, LogLevel::Success);
                            Self::log_static(&logs, "=".repeat(80), LogLevel::Success);
                            Self::log_static(&logs, "RECORD FOUND! STOPPING ALL THREADS".to_string(), LogLevel::Success);
                            Self::log_static(&logs, "=".repeat(80), LogLevel::Success);

                            Self::save_response(&vehicle_no, &date_str, &response, thread_id, status, &results_dir, &logs, &found_count);

                            let preview = response.chars().take(300).collect::<String>()
                            .replace('\n', " ").replace('\t', " ");
                            Self::log_static(&logs, format!("Preview: {}...", preview), LogLevel::Success);

                            record_found.store(true, Ordering::SeqCst);
                            break;
                        }
                }
                Err(e) => {
                    let msg = format!("Thread {}: Error checking {} - {}", thread_id, date_str, e);
                    Self::log_static(&logs, msg, LogLevel::Error);
                }
            }

            current_date = current_date + Duration::days(1);
        }

        if record_found.load(Ordering::SeqCst) {
            Self::log_static(&logs, format!("Thread {}: Stopped due to record found", thread_id), LogLevel::Warning);
        } else {
            Self::log_static(&logs, format!("Thread {}: Completed - Checked {} dates", thread_id, checked_count), LogLevel::Warning);
        }
    }

    fn make_request(vehicle_no: &str, date_str: &str) -> Result<(u16, String), Box<dyn std::error::Error>> {
        let boundary = "wL36Yn8afVp8Ag7AmP8qZ0SA4n1v9T";

        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=registrationNo;\r\n");
        body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
        body.extend_from_slice(vehicle_no.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=registrationDate;\r\n");
        body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
        body.extend_from_slice(date_str.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

        let response = client
        .post("http://58.65.189.226:8080/ovd/API_FOR_VEH_REG_DATA/VEHDATA.php")
        .header("Content-Type", format!("multipart/form-data; boundary={}", boundary))
        .body(body)
        .send()?;

        let status = response.status().as_u16();
        let text = response.text()?;

        Ok((status, text))
    }

    fn save_response(
        vehicle_no: &str,
        date_str: &str,
        response: &str,
        thread_id: usize,
        status: u16,
        results_dir: &PathBuf,
        logs: &Arc<Mutex<Vec<LogEntry>>>,
        found_count: &Arc<Mutex<usize>>,
    ) {
        if let Ok(mut count) = found_count.lock() {
            *count += 1;
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let status_prefix = if status != 200 {
            format!("HTTP{}_", status)
        } else {
            String::new()
        };
        let filename = format!("{}{}_{}.html", status_prefix, vehicle_no, date_str);
        let filepath = results_dir.join(filename.clone());

        match fs::write(&filepath, response) {
            Ok(_) => {
                let msg = format!("Thread {}: Response saved to: {}", thread_id, filename);
                Self::log_static(logs, msg, LogLevel::Success);
            }
            Err(e) => {
                let msg = format!("Thread {}: Error saving file - {}", thread_id, e);
                Self::log_static(logs, msg, LogLevel::Error);
            }
        }
    }

    fn stop_checking(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self.log("Stopping all threads...".to_string(), LogLevel::Warning);
    }
}

impl eframe::App for VehicleChecker {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update status
        let is_running = self.is_running.load(Ordering::SeqCst);
        let record_found = self.record_found.load(Ordering::SeqCst);
        let found_count = *self.found_count.lock().unwrap();
        let checked_dates = *self.checked_dates.lock().unwrap();
        let total_dates = *self.total_dates.lock().unwrap();

        let progress = if total_dates > 0 {
            checked_dates as f32 / total_dates as f32
        } else {
            0.0
        };

        if !is_running && record_found && found_count > 0 {
            self.status_text = "RECORD FOUND!".to_string();
        } else if !is_running {
            self.status_text = "Ready".to_string();
        } else {
            self.status_text = format!("Running... ({}/{})", checked_dates, total_dates);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Vehicle Registration Checker");
            ui.add_space(10.0);

            // Configuration - Centered and Full Width
            ui.vertical_centered(|ui| {
                egui::Frame::group(ui.style())
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.heading("Configuration");
                    });
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.label("Vehicle Registration No:");
                        ui.add(egui::TextEdit::singleline(&mut self.vehicle_no).desired_width(200.0));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Starting Date (YYYY-MM-DD):");
                        ui.add(egui::TextEdit::singleline(&mut self.start_date).desired_width(200.0));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Ending Date (YYYY-MM-DD):");
                        ui.add(egui::TextEdit::singleline(&mut self.end_date).desired_width(200.0));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Number of Threads:");
                        ui.add(egui::Slider::new(&mut self.num_threads, 1..=20));
                    });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!is_running, egui::Button::new("Start")).clicked() {
                            self.start_checking();
                        }

                        if ui.add_enabled(is_running, egui::Button::new("Stop")).clicked() {
                            self.stop_checking();
                        }

                        if ui.button("Clear Console").clicked() {
                            self.clear_logs();
                        }
                    });
                });
            });

            ui.add_space(10.0);

            // Status - Centered and Full Width
            ui.vertical_centered(|ui| {
                egui::Frame::group(ui.style())
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.heading("Status");
                    });
                    ui.add_space(5.0);

                    let color = if record_found && found_count > 0 {
                        egui::Color32::GREEN
                    } else if is_running {
                        egui::Color32::from_rgb(255, 165, 0)
                    } else {
                        egui::Color32::BLUE
                    };

                    ui.vertical_centered(|ui| {
                        ui.colored_label(color, &self.status_text);
                    });

                    ui.add_space(5.0);

                    if is_running || progress > 0.0 {
                        let progress_text = format!("{:.1}% ({}/{})",
                                                    progress * 100.0, checked_dates, total_dates);
                        ui.add(
                            egui::ProgressBar::new(progress)
                            .show_percentage()
                            .text(progress_text)
                        );
                    }
                });
            });

            ui.add_space(10.0);

            // Console
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Console Output");
                ui.add_space(5.0);

                egui::ScrollArea::vertical()
                .max_height(400.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if let Ok(logs) = self.logs.lock() {
                        for entry in logs.iter() {
                            let color = match entry.level {
                                LogLevel::Info => egui::Color32::LIGHT_BLUE,
                                LogLevel::Success => egui::Color32::GREEN,
                                LogLevel::Error => egui::Color32::RED,
                                LogLevel::Warning => egui::Color32::from_rgb(255, 165, 0),
                            };
                            ui.colored_label(color, format!("[{}] {}", entry.timestamp, entry.message));
                        }
                    }
                });
            });
        });

        // Request repaint if running
        if is_running {
            ctx.request_repaint();
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
        .with_inner_size([800.0, 600.0])
        .with_title("Vehicle Registration Checker"),
        ..Default::default()
    };

    eframe::run_native(
        "Vehicle Registration Checker",
        options,
        Box::new(|_cc| Ok(Box::new(VehicleChecker::default()))),
    )
}
