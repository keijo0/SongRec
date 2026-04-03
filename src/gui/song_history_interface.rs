use crate::utils::csv_song_history::SongHistoryRecord;
use log::error;
use std::error::Error;

pub struct RecognitionHistoryInterface {
    csv_path: String,
    pub records: Vec<SongHistoryRecord>,
}

impl RecognitionHistoryInterface {
    pub fn new(
        get_csv_path: fn() -> Result<String, Box<dyn Error>>,
    ) -> Result<Self, Box<dyn Error>> {
        let csv_path = get_csv_path()?;
        let mut interface = RecognitionHistoryInterface {
            csv_path,
            records: Vec::new(),
        };
        if let Err(e) = interface.load() {
            error!("Error reading song history: {}", e);
        }
        Ok(interface)
    }

    pub fn load(&mut self) -> Result<(), Box<dyn Error>> {
        match csv::ReaderBuilder::new()
            .flexible(true)
            .from_path(&self.csv_path)
        {
            Ok(mut reader) => {
                let mut deserialized_records: Vec<csv::Result<SongHistoryRecord>> =
                    reader.deserialize().collect();
                fn item_date(
                    item: &csv::Result<SongHistoryRecord>,
                ) -> Option<chrono::NaiveDateTime> {
                    let s = &item.as_ref().ok()?.recognition_date;
                    chrono::NaiveDateTime::parse_from_str(s, "%c").ok()
                }
                deserialized_records.sort_by_cached_key(item_date);
                self.records = deserialized_records.into_iter().filter_map(|r| r.ok()).collect();
                self.records.reverse(); // newest first
            }
            _ => {}
        }
        Ok(())
    }

    pub fn wipe_and_save(&mut self) {
        self.records.clear();
        if let Ok(mut writer) = csv::Writer::from_path(&self.csv_path) {
            writer.flush().ok();
        }
    }

    pub fn add_row_and_save(&mut self, record: SongHistoryRecord) {
        self.records.insert(0, record);
        self.save();
    }

    pub fn save(&self) {
        if let Ok(mut writer) = csv::Writer::from_path(&self.csv_path) {
            for record in self.records.iter().rev() {
                writer.serialize(record).ok();
            }
            writer.flush().ok();
        }
    }

    pub fn remove(&mut self, record: &SongHistoryRecord) {
        self.records.retain(|r| {
            r.song_name != record.song_name || r.recognition_date != record.recognition_date
        });
        self.save();
    }
}

#[test]
fn test_item_date() {
    let s = "Sat Aug 17 22:44:43 2024";
    let parsed = chrono::NaiveDateTime::parse_from_str(s, "%c").unwrap();
    assert_eq!(&parsed.format("%c").to_string(), s);
}
