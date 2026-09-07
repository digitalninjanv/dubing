use crate::domain::{JobProgress, PipelineStage};
use gtk4::prelude::*;

#[derive(Clone)]
pub struct ProgressView {
    scroller: gtk4::ScrolledWindow,
    progress_bar: gtk4::ProgressBar,
    stage_label: gtk4::Label,
    detail_label: gtk4::Label,
    cancel_btn: gtk4::Button,
    stage_rows: Vec<(PipelineStage, gtk4::Label, gtk4::Image)>,
}

impl Default for ProgressView {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressView {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_margin_top(32);
        container.set_margin_bottom(32);
        container.set_margin_start(40);
        container.set_margin_end(40);

        let title = gtk4::Label::new(Some("Processing Audio"));
        title.add_css_class("title-2");
        container.append(&title);

        let stage_label = gtk4::Label::new(Some("Initializing…"));
        stage_label.add_css_class("heading");
        stage_label.set_margin_top(8);
        container.append(&stage_label);

        let progress_bar = gtk4::ProgressBar::new();
        progress_bar.set_fraction(0.0);
        progress_bar.set_margin_top(8);
        progress_bar.set_margin_bottom(8);
        container.append(&progress_bar);

        let detail_label = gtk4::Label::new(None);
        detail_label.add_css_class("dim-label");
        detail_label.set_wrap(true);
        container.append(&detail_label);

        // Stage Step List Card
        let steps_frame = gtk4::Frame::new(None);
        steps_frame.add_css_class("card");
        steps_frame.set_margin_top(16);

        let steps_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        steps_box.set_margin_top(16);
        steps_box.set_margin_bottom(16);
        steps_box.set_margin_start(16);
        steps_box.set_margin_end(16);

        let stages = [
            (PipelineStage::Validating, "Validating audio"),
            (PipelineStage::Uploading, "Uploading audio to Gemini"),
            (
                PipelineStage::Transcribing,
                "Transcribing speech & speaker diarization",
            ),
            (PipelineStage::Translating, "Translating transcript"),
            (PipelineStage::Synthesizing, "Generating voice audio"),
            (PipelineStage::Aligning, "Aligning speech timeline"),
            (PipelineStage::Exporting, "Encoding final MP3 output"),
            (PipelineStage::ValidatingOutput, "Validating output file"),
        ];

        let mut stage_rows = Vec::new();

        for (stage, label_text) in stages {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            let icon = gtk4::Image::from_icon_name("radio-symbolic");
            icon.add_css_class("dim-label");

            let label = gtk4::Label::new(Some(label_text));
            label.set_halign(gtk4::Align::Start);
            label.set_hexpand(true);

            row.append(&icon);
            row.append(&label);
            steps_box.append(&row);

            stage_rows.push((stage, label, icon));
        }

        steps_frame.set_child(Some(&steps_box));
        container.append(&steps_frame);

        let cancel_btn = gtk4::Button::with_label("Cancel");
        cancel_btn.add_css_class("destructive-action");
        cancel_btn.add_css_class("pill");
        cancel_btn.set_halign(gtk4::Align::Center);
        cancel_btn.set_margin_top(24);
        container.append(&cancel_btn);

        let clamp = libadwaita::Clamp::new();
        clamp.set_maximum_size(780);
        clamp.set_tightening_threshold(580);
        clamp.set_vexpand(true);
        clamp.set_child(Some(&container));

        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scroller.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
        scroller.set_propagate_natural_width(false);
        scroller.set_propagate_natural_height(false);
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);
        scroller.set_child(Some(&clamp));

        Self {
            scroller,
            progress_bar,
            stage_label,
            detail_label,
            cancel_btn,
            stage_rows,
        }
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scroller
    }

    pub fn update_detail(&self, detail: &str) {
        self.detail_label.set_text(detail);
    }

    pub fn update_progress(&self, progress: &JobProgress) {
        self.progress_bar.set_fraction(progress.fraction());
        self.stage_label
            .set_text(progress.current_stage.display_label());
        self.detail_label.set_text(&progress.message);

        for (stage, label, icon) in &self.stage_rows {
            if progress.current_stage == *stage {
                icon.set_icon_name(Some("emblem-synchronizing-symbolic"));
                icon.remove_css_class("dim-label");
                icon.add_css_class("accent");
                label.add_css_class("bold");
            } else if progress.fraction() > 0.0
                && self.is_stage_completed(*stage, progress.current_stage)
            {
                icon.set_icon_name(Some("emblem-ok-symbolic"));
                icon.remove_css_class("dim-label");
                icon.remove_css_class("accent");
                icon.add_css_class("success");
                label.remove_css_class("bold");
            } else {
                icon.set_icon_name(Some("radio-symbolic"));
                icon.remove_css_class("accent");
                icon.remove_css_class("success");
                icon.add_css_class("dim-label");
                label.remove_css_class("bold");
            }
        }
    }

    fn is_stage_completed(
        &self,
        target_stage: PipelineStage,
        current_stage: PipelineStage,
    ) -> bool {
        let order = [
            PipelineStage::Validating,
            PipelineStage::Uploading,
            PipelineStage::Transcribing,
            PipelineStage::Translating,
            PipelineStage::Synthesizing,
            PipelineStage::Aligning,
            PipelineStage::Exporting,
            PipelineStage::ValidatingOutput,
            PipelineStage::Completed,
        ];

        let target_pos = order.iter().position(|s| *s == target_stage);
        let current_pos = order.iter().position(|s| *s == current_stage);

        match (target_pos, current_pos) {
            (Some(t), Some(c)) => c > t,
            _ => false,
        }
    }

    pub fn connect_cancel<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.cancel_btn.connect_clicked(move |_| callback());
    }
}
