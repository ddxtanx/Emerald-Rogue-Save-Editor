//! Trainer tab: TID/SID, name, playtime.

use super::*;

impl Editor {
    pub(crate) fn trainer_ui(&mut self, ui: &mut egui::Ui) {
        let Some(sf) = &self.file else { return };
        let mut name = sf.player_name();
        let mut tid = sf.trainer_id();
        let mut sid = sf.secret_id();
        let mut money = sf.money();
        let (mut h, mut mnt, mut sec) = sf.play_time();
        let mut changed = false;
        egui::Grid::new("trainer").num_columns(2).show(ui, |ui| {
            ui.label("Player name");
            if ui.add(egui::TextEdit::singleline(&mut name)).changed() {
                changed = true;
            }
            ui.end_row();
            ui.label("Trainer ID");
            if ui
                .add(egui::DragValue::new(&mut tid).clamp_range(0..=65535))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Secret ID");
            if ui
                .add(egui::DragValue::new(&mut sid).clamp_range(0..=65535))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Money");
            if ui
                .add(egui::DragValue::new(&mut money).clamp_range(0..=9999999))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Play time");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut h)
                            .clamp_range(0..=9999)
                            .suffix(" h"),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut mnt)
                            .clamp_range(0..=59)
                            .suffix(" m"),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut sec)
                            .clamp_range(0..=59)
                            .suffix(" s"),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.end_row();
        });
        if changed {
            if let Some(sf) = self.file.as_mut() {
                sf.set_player_name(&name);
                sf.set_tids(tid, sid);
                sf.set_money(money);
            }
            self.dirty = true;
        }
    }
}
