use super::*;

pub const BG: Color32 = Color32::from_rgb(0x0e, 0x06, 0x05);
pub const PANEL: Color32 = Color32::from_rgba_unmultiplied_const(26, 10, 7, 235);
pub const PANEL_HI: Color32 = Color32::from_rgb(0x2a, 0x10, 0x0a);
pub const BORDER: Color32 = Color32::from_rgb(0x5a, 0x1d, 0x10);
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0x4a, 0x1c);
pub const ACCENT_2: Color32 = Color32::from_rgb(0xff, 0x7a, 0x2c);
pub const TEXT: Color32 = Color32::from_rgb(0xff, 0xe6, 0xda);
pub const MUTED: Color32 = Color32::from_rgb(0xb9, 0x8f, 0x80);
pub const SUCCESS: Color32 = Color32::from_rgb(0x4a, 0xde, 0x80);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x4d, 0x4d);
pub const BOYS: Color32 = Color32::from_rgb(0x4a, 0x90, 0xff);
pub const GIRLS: Color32 = Color32::from_rgb(0xff, 0x66, 0xcc);
const SCRIM: Color32 = Color32::from_black_alpha(150);
const SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 10],
    blur: 30,
    spread: 0,
    color: Color32::from_black_alpha(170),
};
const PANEL_RADIUS: u8 = 12;
const WIDGET_RADIUS: u8 = 8;
const CAPTION: f32 = 12.0;
const SMALL: f32 = 14.0;
const BODY: f32 = 16.0;
const INPUT: f32 = 18.0;
const BUTTON: f32 = 20.0;
const SECTION: f32 = 28.0;
const TITLE: f32 = 44.0;
const CROSSHAIR_COLORS: [Color32; 5] = [
    Color32::WHITE,
    SUCCESS,
    Color32::from_rgb(255, 223, 64),
    Color32::from_rgb(103, 232, 249),
    ACCENT,
];
const TIERS: [(&str, Color32); 5] = [
    ("Bronz", Color32::from_rgb(0xcd, 0x7f, 0x32)),
    ("Gümüş", Color32::from_rgb(0xc0, 0xc8, 0xd4)),
    ("Altın", Color32::from_rgb(0xff, 0xc9, 0x4a)),
    ("Platin", Color32::from_rgb(0x5e, 0xea, 0xd4)),
    ("Elmas", Color32::from_rgb(0x7d, 0xd3, 0xfc)),
];

pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(MUTED);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL_HI;
    v.text_edit_bg_color = Some(PANEL_HI);
    v.window_corner_radius = PANEL_RADIUS.into();
    v.menu_corner_radius = WIDGET_RADIUS.into();
    v.window_shadow = SHADOW;
    v.popup_shadow = SHADOW;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    v.hyperlink_color = ACCENT_2;
    v.error_fg_color = DANGER;
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, PANEL, BORDER),
        (&mut v.widgets.inactive, PANEL_HI, BORDER),
        (&mut v.widgets.hovered, PANEL_HI, ACCENT),
        (&mut v.widgets.active, ACCENT.gamma_multiply(0.7), ACCENT),
        (&mut v.widgets.open, PANEL_HI, ACCENT),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke = Stroke::new(1.0, TEXT);
        w.corner_radius = WIDGET_RADIUS.into();
    }
    ctx.set_visuals(v);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = vec2(12.0, 12.0);
        s.spacing.button_padding = vec2(16.0, 8.0);
        s.spacing.window_margin = 24.into();
        s.spacing.interact_size = vec2(40.0, 32.0);
        s.spacing.slider_width = 240.0;
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(BODY));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(BODY));
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(CAPTION));
        s.text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(SECTION));
    });
}

fn button(ui: &mut egui::Ui, text: &str, enabled: bool, primary: bool) -> bool {
    let width = ui.available_width().max(320.0);
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, 48.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, enabled && response.hovered(), 0.12);
    let opacity = if enabled { 1.0 } else { 0.4 };
    let fill = if primary {
        ACCENT.gamma_multiply(if response.is_pointer_button_down_on() {
            0.8
        } else {
            1.0 + hover * 0.12
        })
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        WIDGET_RADIUS,
        fill.gamma_multiply(opacity),
        Stroke::new(
            1.0,
            (if hover > 0.1 { ACCENT } else { BORDER }).gamma_multiply(opacity),
        ),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(BUTTON),
        TEXT.gamma_multiply(opacity),
    );
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && response.clicked()
}

fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    button(ui, text, enabled, true)
}
fn secondary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    button(ui, text, enabled, false)
}

fn card<R>(
    ui: &mut egui::Ui,
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(PANEL_RADIUS)
        .shadow(SHADOW)
        .inner_margin(24)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width((width - 48.0).max(0.0));
                contents(ui)
            })
            .inner
        })
}

fn text_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    password: bool,
    error: Option<&str>,
) -> egui::Response {
    if !label.is_empty() {
        ui.label(RichText::new(label).size(SMALL).color(MUTED));
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(label)
            .password(password)
            .font(FontId::proportional(INPUT))
            .desired_width(ui.available_width())
            .min_size(vec2(0.0, 44.0))
            .margin(vec2(12.0, 10.0)),
    );
    if response.has_focus() || error.is_some() {
        ui.painter().rect_stroke(
            response.rect,
            WIDGET_RADIUS,
            Stroke::new(1.0, if error.is_some() { DANGER } else { ACCENT }),
            StrokeKind::Inside,
        );
    }
    if let Some(error) = error {
        ui.label(RichText::new(error).size(13.0).color(DANGER));
    }
    response
}

fn tier(rating: u32) -> (&'static str, Color32) {
    TIERS[match rating {
        0..900 => 0,
        900..1100 => 1,
        1100..1300 => 2,
        1300..1500 => 3,
        _ => 4,
    }]
}
fn tier_badge(ui: &mut egui::Ui, rating: u32) {
    let (name, color) = tier(rating);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(255)
        .inner_margin(egui::Margin::symmetric(12, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{name}  {rating}"))
                    .size(SMALL)
                    .color(color),
            );
        });
}

fn spinner(ui: &mut egui::Ui) {
    ui.add(egui::Spinner::new().size(24.0).color(ACCENT));
}
fn title(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    ui.label(RichText::new(text).size(size).strong().color(color));
}
fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(SMALL).color(MUTED));
}
fn clock(seconds: f64) -> String {
    let s = seconds.ceil().max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl Game {
    fn backdrop(&self, ctx: &egui::Context) {
        let screen = ctx.viewport_rect();
        let p = ctx.layer_painter(LayerId::new(Order::Background, Id::new("menu-scrim")));
        p.rect_filled(screen, 0, SCRIM);
        let center = screen.center() - vec2(0.0, screen.height() * 0.15);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(center, ACCENT.gamma_multiply(0.13));
        for step in 0..=64 {
            let a = step as f32 * std::f32::consts::TAU / 64.0;
            mesh.colored_vertex(
                center + vec2(a.cos() * screen.width(), a.sin() * screen.height()),
                BG.gamma_multiply(0.75),
            );
            if step > 0 {
                mesh.add_triangle(0, step, step + 1);
            }
        }
        p.add(mesh);
    }

    pub(super) fn menu(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Playing {
            return;
        }
        self.backdrop(ctx);
        if self.screen == Screen::Lobby {
            self.lobby_ui(ctx);
            return;
        }
        let viewport = ctx.viewport_rect();
        let width = if self.screen == Screen::Settings {
            520.0
        } else if self.screen == Screen::MatchIntro {
            780.0
        } else {
            420.0_f32
        }
        .min(viewport.width() - 32.0);
        egui::Area::new(Id::new("menu"))
            .default_size(vec2(width, viewport.height() - 32.0))
            .anchor(
                Align2::CENTER_CENTER,
                if self.screen == Screen::Settings {
                    vec2(0.0, -36.0)
                } else {
                    Vec2::ZERO
                },
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_max_height(viewport.height() - 32.0);
                egui::ScrollArea::vertical()
                    .id_salt("screen-scroll")
                    .max_height(
                        viewport.height()
                            - if self.screen == Screen::Settings {
                                120.0
                            } else {
                                32.0
                            },
                    )
                    .show(ui, |ui| {
                        if matches!(self.screen, Screen::Auth | Screen::Boot) {
                            ui.vertical_centered(|ui| {
                                let ratio = self.logo.size_vec2();
                                let h = (viewport.height() * 0.16).min(128.0);
                                ui.image((self.logo.id(), ratio * (h / ratio.y)));
                            });
                            ui.add_space(12.0);
                        }
                        card(ui, width, |ui| {
                            if self.screen == Screen::Auth {
                                self.auth_ui(ui);
                                return;
                            }
                            ui.vertical_centered(|ui| {
                                match self.screen {
                                    Screen::Boot => {
                                        spinner(ui);
                                        title(ui, "Giriş yapılıyor…", SECTION, TEXT);
                                        if secondary_button(ui, "Geri", true) {
                                            self.logout();
                                        }
                                    }
                                    Screen::Queue => {
                                        ui.add(
                                            egui::Spinner::new()
                                                .size(
                                                    56.0 + (self.last_frame as f32 * 0.003).sin()
                                                        * 3.0,
                                                )
                                                .color(ACCENT),
                                        );
                                        title(ui, "Rakip aranıyor…", SECTION, TEXT);
                                        let elapsed = (self.last_frame - self.queue_since) / 1000.0;
                                        title(ui, &clock(elapsed.floor()), TITLE, ACCENT_2);
                                        muted(
                                            ui,
                                            format!(
                                                "±{} puan",
                                                (100 + 50 * (elapsed as u32 / 10)).min(1000)
                                            ),
                                        );
                                        muted(ui, format!("{} oyuncu sırada", self.queued));
                                        if secondary_button(
                                            ui,
                                            "İptal (Esc)",
                                            self.pending.is_none(),
                                        ) {
                                            self.request("UNQUEUE");
                                        }
                                    }
                                    Screen::MatchIntro => {
                                        ui.columns(3, |cols| {
                                            let my_rating =
                                                self.account.as_ref().map_or(1000, |a| a.rating);
                                            let (name, rating) = self
                                                .opponent
                                                .clone()
                                                .unwrap_or(("Rakip".to_owned(), 1000));
                                            cols[0].vertical_centered(|ui| {
                                                title(
                                                    ui,
                                                    &self.username,
                                                    SECTION,
                                                    self.team.unwrap_or(Team::Boys).color(),
                                                );
                                                tier_badge(ui, my_rating);
                                            });
                                            cols[1].vertical_centered(|ui| {
                                                title(ui, "VS", TITLE, MUTED)
                                            });
                                            cols[2].vertical_centered(|ui| {
                                                title(
                                                    ui,
                                                    &name,
                                                    SECTION,
                                                    if self.team == Some(Team::Boys) {
                                                        GIRLS
                                                    } else {
                                                        BOYS
                                                    },
                                                );
                                                tier_badge(ui, rating);
                                            });
                                        });
                                        let remaining =
                                            ((self.go_at - self.last_frame) / 1000.0).max(0.0);
                                        let pulse = (1.0 - remaining.fract()) as f32;
                                        title(
                                            ui,
                                            if self.ranked_started { "BAŞLA" } else { "" },
                                            SECTION,
                                            ACCENT,
                                        );
                                        if !self.ranked_started {
                                            title(
                                                ui,
                                                &format!("{:.0}", remaining.ceil()),
                                                120.0 + pulse * 8.0,
                                                ACCENT_2,
                                            );
                                        }
                                        muted(
                                            ui,
                                            format!("İlk {} leşe ulaşan kazanır", self.kill_target),
                                        );
                                        if !self.grabbed
                                            && primary_button(ui, "Başlamak için tıkla", true)
                                        {
                                            self.lock_pointer();
                                        }
                                        if secondary_button(
                                            ui,
                                            "Maçtan ayrıl",
                                            self.pending.is_none(),
                                        ) {
                                            if self.ranked_started {
                                                self.confirm_leave = true;
                                                self.show_screen(Screen::Paused);
                                            } else {
                                                self.lobby();
                                            }
                                        }
                                    }
                                    Screen::Paused => {
                                        title(ui, "DURAKLATILDI", SECTION, TEXT);
                                        if self.confirm_leave {
                                            title(
                                                ui,
                                                "Ayrılmak mağlubiyet sayılır",
                                                BUTTON,
                                                DANGER,
                                            );
                                            if primary_button(
                                                ui,
                                                "Ayrılmayı onayla",
                                                self.pending.is_none(),
                                            ) {
                                                self.confirm_leave = false;
                                                self.lobby();
                                            }
                                            if secondary_button(
                                                ui,
                                                "Vazgeç",
                                                self.pending.is_none(),
                                            ) {
                                                self.confirm_leave = false;
                                            }
                                        } else {
                                            if primary_button(
                                                ui,
                                                "Devam et",
                                                self.pending.is_none(),
                                            ) {
                                                self.escape();
                                            }
                                            if secondary_button(
                                                ui,
                                                "Ayarlar",
                                                self.pending.is_none(),
                                            ) {
                                                self.settings_back = Screen::Paused;
                                                self.show_screen(Screen::Settings);
                                            }
                                            if secondary_button(
                                                ui,
                                                "Maçtan ayrıl",
                                                self.pending.is_none(),
                                            ) {
                                                if self.mode == Mode::Ranked && self.ranked_started
                                                {
                                                    self.confirm_leave = true;
                                                } else {
                                                    self.lobby();
                                                }
                                            }
                                        }
                                    }
                                    Screen::Settings => {
                                        title(ui, "AYARLAR", SECTION, TEXT);
                                        self.settings(ui);
                                    }
                                    Screen::Dead => {
                                        title(ui, "ÖLDÜN", TITLE, DANGER);
                                        ui.label(format!(
                                            "Seni öldürdü: {}",
                                            self.killed_by.as_deref().unwrap_or("Rakip")
                                        ));
                                        if self.mode == Mode::Ranked {
                                            title(
                                                ui,
                                                &format!(
                                                    "Yeniden doğuş: {:.0}",
                                                    ((self.died_at + 2000.0 - self.last_frame)
                                                        / 1000.0)
                                                        .ceil()
                                                        .max(0.0)
                                                ),
                                                SECTION,
                                                ACCENT_2,
                                            );
                                        } else if primary_button(
                                            ui,
                                            "Yeniden doğ",
                                            self.pending.is_none(),
                                        ) {
                                            self.lock_pointer();
                                            self.request("RESPAWN");
                                        }
                                        if secondary_button(
                                            ui,
                                            "Maçtan ayrıl",
                                            self.pending.is_none(),
                                        ) {
                                            if self.mode == Mode::Ranked && self.ranked_started {
                                                self.confirm_leave = true;
                                                self.show_screen(Screen::Paused);
                                            } else {
                                                self.lobby();
                                            }
                                        }
                                    }
                                    Screen::Result => {
                                        let won = self.team == self.winner;
                                        title(
                                            ui,
                                            if won { "ZAFER" } else { "YENİLGİ" },
                                            64.0,
                                            if won { SUCCESS } else { DANGER },
                                        );
                                        title(
                                            ui,
                                            &format!("{} — {}", self.score[0], self.score[1]),
                                            TITLE,
                                            TEXT,
                                        );
                                        muted(
                                            ui,
                                            format!(
                                                "Yeniden başlıyor: {:.0}",
                                                ((self.result_deadline - self.last_frame) / 1000.0)
                                                    .ceil()
                                                    .max(0.0)
                                            ),
                                        );
                                        if secondary_button(
                                            ui,
                                            "Lobiye dön",
                                            self.pending.is_none(),
                                        ) {
                                            self.lobby();
                                        }
                                    }
                                    Screen::RankedResult => self.ranked_result_ui(ui),
                                    Screen::Disconnected => {
                                        title(ui, "BAĞLANTI KESİLDİ", SECTION, DANGER);
                                        muted(ui, "Oturumuna yeniden bağlanabilirsin.");
                                        if primary_button(
                                            ui,
                                            "Yeniden bağlan",
                                            self.pending.is_none(),
                                        ) {
                                            if self.token.is_empty() {
                                                self.show_screen(Screen::Auth);
                                                self.focus_auth = true;
                                            } else if let Err(error) =
                                                self.connect(format!("TOKEN|{}", self.token), true)
                                            {
                                                self.status = error;
                                            }
                                        }
                                        if secondary_button(ui, "Çıkış", true) {
                                            self.logout();
                                        }
                                    }
                                    _ => {}
                                }
                                if self.pending.is_some()
                                    && !matches!(self.screen, Screen::Boot | Screen::Queue)
                                {
                                    spinner(ui);
                                }
                                if !self.status.is_empty()
                                    && !matches!(
                                        self.screen,
                                        Screen::Result | Screen::Dead | Screen::Boot
                                    )
                                {
                                    muted(ui, self.status.clone());
                                }
                            });
                        });
                    });
            });
        if self.screen == Screen::Settings {
            egui::Area::new(Id::new("settings-back"))
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -16.0))
                .show(ctx, |ui| {
                    ui.set_width(472.0);
                    egui::Frame::new()
                        .fill(PANEL)
                        .corner_radius(WIDGET_RADIUS)
                        .show(ui, |ui| {
                            if secondary_button(ui, "Geri (Esc)", true) {
                                self.show_screen(self.settings_back);
                            }
                        });
                });
        }
    }

    fn auth_ui(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        let waiting = self.pending.is_some();
        ui.add_enabled_ui(!waiting, |ui| {
            ui.horizontal(|ui| {
                for (register, label) in [(false, "Giriş Yap"), (true, "Kayıt Ol")] {
                    let r = ui.selectable_label(
                        self.register_tab == register,
                        RichText::new(label).size(BUTTON),
                    );
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if self.register_tab == register {
                        ui.painter().line_segment(
                            [r.rect.left_bottom(), r.rect.right_bottom()],
                            Stroke::new(2.0, ACCENT),
                        );
                    }
                    if r.clicked() {
                        self.register_tab = register;
                        self.touched = [false; 3];
                        self.field_error = None;
                        self.status.clear();
                        self.focus_auth = true;
                    }
                }
            });
            ui.add_space(4.0);
            let error = |field| {
                self.field_error
                    .as_ref()
                    .filter(|(f, _)| *f == field)
                    .map(|(_, e)| e.clone())
            };
            let name_error = error(Field::Username);
            let password_error = error(Field::Password);
            let confirm_error = error(Field::Confirm);
            let response = text_field(
                ui,
                "Kullanıcı adı",
                &mut self.username,
                false,
                name_error.as_deref(),
            );
            if self.focus_auth {
                response.request_focus();
                self.focus_auth = false;
            }
            if response.lost_focus() {
                self.touched[0] = true;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Şifre").size(SMALL).color(MUTED));
                if ui
                    .small_button(if self.show_password {
                        "gizle"
                    } else {
                        "göster"
                    })
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.show_password = !self.show_password;
                }
            });
            // The shared field keeps the password toggle adjacent to its label.
            let response = text_field(
                ui,
                "",
                &mut self.password,
                !self.show_password,
                password_error.as_deref(),
            );
            if response.lost_focus() {
                self.touched[1] = true;
            }
            if self.register_tab
                && text_field(
                    ui,
                    "Şifre tekrar",
                    &mut self.confirm,
                    !self.show_password,
                    confirm_error.as_deref(),
                )
                .lost_focus()
            {
                self.touched[2] = true;
            }
            self.field_error = self.validate_auth();
            ui.checkbox(&mut self.remember, "Beni hatırla")
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        });
        if !self.status.is_empty() {
            ui.label(
                RichText::new(&self.status)
                    .color(if waiting { MUTED } else { DANGER })
                    .size(SMALL),
            );
        }
        if waiting {
            ui.horizontal(|ui| {
                spinner(ui);
                muted(ui, "Sunucu yanıtı bekleniyor…");
            });
        }
        if primary_button(
            ui,
            if self.register_tab {
                "KAYIT OL"
            } else {
                "GİRİŞ YAP"
            },
            !waiting,
        ) || (!waiting && ui.input(|i| i.key_pressed(Key::Enter)))
        {
            self.submit_auth();
        }
        ui.label(
            RichText::new("3–16 harf, rakam, _ veya -")
                .size(CAPTION)
                .color(MUTED),
        );
    }

    fn lobby_ui(&mut self, ctx: &egui::Context) {
        let screen = ctx.viewport_rect();
        let enabled = self.pending.is_none();
        egui::Area::new(Id::new("lobby-top"))
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(24, 8))
                    .show(ui, |ui| {
                        ui.set_width(screen.width() - 48.0);
                        ui.horizontal(|ui| {
                            let size = self.logo.size_vec2();
                            ui.image((self.logo.id(), size * (48.0 / size.y)));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(enabled, egui::Button::new("Çıkış yap"))
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.logout();
                                    }
                                    if ui
                                        .add_enabled(enabled, egui::Button::new("Ayarlar"))
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.settings_back = Screen::Lobby;
                                        self.show_screen(Screen::Settings);
                                    }
                                    if let Some(a) = &self.account {
                                        tier_badge(ui, a.rating);
                                        ui.label(RichText::new(&a.name).strong());
                                    }
                                    ui.label(
                                        RichText::new(format!("● {} çevrimiçi", self.online))
                                            .size(SMALL)
                                            .color(SUCCESS),
                                    );
                                },
                            );
                        });
                    });
            });
        let narrow = screen.width() < 1100.0;
        let outer = screen.width() - 48.0;
        let mode_width = if narrow {
            (outer - 16.0) / 2.0
        } else {
            (outer - 280.0 - 32.0) / 2.0
        };
        egui::Area::new(Id::new("lobby-content")).default_size(vec2(outer,screen.height()-100.0)).fixed_pos(screen.min+vec2(24.0,88.0)).show(ctx,|ui|{
            ui.set_width(outer);
            ui.set_max_height(screen.height()-100.0);
            egui::ScrollArea::vertical().id_salt("lobby-scroll").max_height(screen.height()-100.0).show(ui,|ui|{
                ui.horizontal_top(|ui|{
                    ui.spacing_mut().item_spacing.x=16.0;
                    self.mode_card(ui,mode_width,false);
                    self.mode_card(ui,mode_width,true);
                    if !narrow {self.leaderboard_ui(ui,280.0,220.0);}
                });
                if narrow {self.leaderboard_ui(ui,outer,68.0);}
                muted(ui,"WASD hareket · Fare nişan · Boşluk zıpla · Sol tık ateş · Tab skor · Esc menü");
                if self.pending.is_some(){ui.horizontal(|ui|{spinner(ui);muted(ui,"Odaya bağlanılıyor…");});}
            });
        });
    }

    fn mode_card(&mut self, ui: &mut egui::Ui, width: f32, ranked: bool) {
        let id = ui.id().with(if ranked { "ranked-card" } else { "dm-card" });
        let hovered = ui.ctx().pointer_hover_pos().is_some_and(|p| {
            Rect::from_min_size(ui.next_widget_position(), vec2(width, 252.0)).contains(p)
        });
        let lift = ui.ctx().animate_bool_with_time(id, hovered, 0.12) * 2.0;
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.add_space(2.0 - lift);
            let response = card(ui, width, |ui| {
                ui.set_min_height(204.0);
                ui.add_space(4.0);
                muted(
                    ui,
                    if ranked {
                        "REKABET · 1v1"
                    } else {
                        "TAKIM · 8v8"
                    },
                );
                title(
                    ui,
                    if ranked {
                        "DERECELİ 1v1"
                    } else {
                        "TAKIM ÖLÜM MAÇI"
                    },
                    24.0,
                    TEXT,
                );
                if ranked {
                    muted(ui, "İlk 10 leş · 5 dakika");
                    if let Some(a) = &self.account {
                        ui.horizontal(|ui| {
                            tier_badge(ui, a.rating);
                            muted(ui, format!("{}G – {}M", a.wins, a.losses));
                        });
                        let total = a.wins as f64 + a.losses as f64;
                        muted(
                            ui,
                            format!(
                                "%{:.0} kazanma",
                                if total > 0.0 {
                                    a.wins as f64 / total * 100.0
                                } else {
                                    0.0
                                }
                            ),
                        );
                    }
                    if primary_button(ui, "DERECELİ MAÇ BUL", self.pending.is_none()) {
                        self.request("QUEUE");
                    }
                } else {
                    muted(ui, format!("Erkekler ve Kızlar · {} leş", map::WIN_SCORE));
                    title(
                        ui,
                        &format!("{} / 16 oyuncu", self.dm_players),
                        SECTION,
                        ACCENT_2,
                    );
                    muted(ui, "Takımını zafere taşı.");
                    if primary_button(ui, "OYNA", self.pending.is_none()) {
                        self.mode = Mode::Deathmatch;
                        self.clear_match();
                        self.lock_pointer();
                        self.request("PLAY");
                    }
                }
            });
            let rect = response.response.rect;
            let start = rect.left_top();
            if hovered {
                ui.painter().rect_stroke(
                    rect,
                    PANEL_RADIUS,
                    Stroke::new(1.0, ACCENT),
                    StrokeKind::Inside,
                );
            }
            let rating = self.account.as_ref().map_or(1000, |a| a.rating);
            let mut stripe = egui::Mesh::default();
            let (left, right) = if ranked {
                (tier(rating).1, tier(rating).1.gamma_multiply(0.2))
            } else {
                (BOYS, GIRLS)
            };
            for (p, c) in [
                (start + vec2(12.0, 0.0), left),
                (start + vec2(width - 12.0, 0.0), right),
                (start + vec2(width - 12.0, 3.0), right),
                (start + vec2(12.0, 3.0), left),
            ] {
                stripe.colored_vertex(p, c);
            }
            stripe.add_triangle(0, 1, 2);
            stripe.add_triangle(0, 2, 3);
            ui.painter().add(stripe);
            ui.add_space(lift);
        });
    }

    fn leaderboard_ui(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        card(ui, width, |ui| {
            title(ui, "EN İYİ 10", BUTTON, ACCENT_2);
            egui::ScrollArea::vertical()
                .id_salt("leaderboard")
                .max_height(height)
                .show(ui, |ui| {
                    if self.top.is_empty() {
                        muted(ui, "İlk dereceli sonucu sen yaz.");
                    }
                    for (i, (name, rating, _, _)) in self.top.iter().enumerate() {
                        egui::Frame::new()
                            .fill(if name == &self.username {
                                PANEL_HI
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(WIDGET_RADIUS)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{:>2}  {name}", i + 1)).color(
                                            if name == &self.username { ACCENT } else { TEXT },
                                        ),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(rating.to_string())
                                                    .color(tier(*rating).1),
                                            );
                                        },
                                    );
                                });
                            });
                    }
                });
        });
    }

    fn ranked_result_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(r) = &self.ranked_result {
            let color = match r.result.as_str() {
                "win" => SUCCESS,
                "loss" => DANGER,
                _ => ACCENT_2,
            };
            title(
                ui,
                match r.result.as_str() {
                    "win" => "ZAFER",
                    "loss" => "YENİLGİ",
                    _ => "BERABERE",
                },
                64.0,
                color,
            );
            muted(
                ui,
                match r.reason.as_str() {
                    "forfeit" if r.result == "win" => "Rakip ayrıldı",
                    "forfeit" => "Maçtan ayrıldın",
                    "cancelled" => "Maç başlamadan iptal edildi",
                    "time" => "Süre doldu",
                    _ => "Leş hedefine ulaşıldı",
                },
            );
            let t = ((self.last_frame - r.at) / 1200.0).clamp(0.0, 1.0);
            let rating = (r.old as f64
                + (r.rating as f64 - r.old as f64) * (1.0 - (1.0 - t).powi(3)))
            .round() as u32;
            title(ui, &rating.to_string(), TITLE, TEXT);
            egui::Frame::new()
                .fill(color.gamma_multiply(0.15))
                .corner_radius(255)
                .inner_margin(12)
                .show(ui, |ui| {
                    title(ui, &format!("{:+}", r.delta), SECTION, color)
                });
            tier_badge(ui, r.rating);
            if tier(r.old).0 != tier(r.rating).0 {
                title(
                    ui,
                    &format!("Yeni lig: {}", tier(r.rating).0),
                    BUTTON,
                    tier(r.rating).1,
                );
            }
        }
        if primary_button(ui, "Tekrar sıraya gir", self.pending.is_none()) {
            self.request("QUEUE");
        }
        if secondary_button(ui, "Lobiye dön", self.pending.is_none()) {
            self.clear_match();
            self.show_screen(Screen::Lobby);
            self.send("INFO".to_owned());
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let before = self.settings_values();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            title(ui, "Kontroller", BUTTON, ACCENT_2);
            ui.add(
                egui::Slider::new(&mut self.sensitivity, 0.0..=200.0)
                    .text("Hassasiyet")
                    .suffix(" %")
                    .step_by(1.0),
            );
            title(ui, "Görüntü", BUTTON, ACCENT_2);
            ui.horizontal(|ui| {
                ui.label("Çözünürlük");
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(
                        self.resolution
                            .map_or("Ekrana uyarla".to_owned(), |[w, h]| format!("{w} × {h}")),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.resolution, None, "Ekrana uyarla");
                        let monitor = ui.ctx().input(|i| {
                            i.viewport().monitor_size.map(|size| {
                                size * i.viewport().native_pixels_per_point.unwrap_or(1.0)
                            })
                        });
                        for [w, h] in RESOLUTIONS {
                            if monitor.is_none_or(|m| w as f32 <= m.x && h as f32 <= m.y) {
                                ui.selectable_value(
                                    &mut self.resolution,
                                    Some([w, h]),
                                    format!("{w} × {h}"),
                                );
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("FPS sınırı");
                egui::ComboBox::from_id_salt("fps-limit")
                    .selected_text(if self.fps_limit == 0 {
                        "Sınırsız".to_owned()
                    } else {
                        self.fps_limit.to_string()
                    })
                    .show_ui(ui, |ui| {
                        for fps in FPS_LIMITS {
                            ui.selectable_value(
                                &mut self.fps_limit,
                                fps,
                                if fps == 0 {
                                    "Sınırsız".to_owned()
                                } else {
                                    fps.to_string()
                                },
                            );
                        }
                    });
                ui.checkbox(&mut self.show_fps, "FPS göster")
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
            });
            if ui
                .checkbox(&mut self.fullscreen, "Tam ekran")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .changed()
            {
                self.apply_display();
            }
            title(ui, "Arayüz", BUTTON, ACCENT_2);
            ui.add(
                egui::Slider::new(&mut self.hud_scale, 50.0..=200.0)
                    .text("Arayüz ölçeği")
                    .suffix(" %")
                    .step_by(10.0),
            );
            ui.horizontal(|ui| {
                ui.label("Nişangâh");
                for (style, name) in ["Artı", "Nokta", "Artı + nokta"].iter().enumerate() {
                    ui.selectable_value(&mut self.crosshair.0, style, *name)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
            ui.add(
                egui::Slider::new(&mut self.crosshair.1, 3.0..=20.0)
                    .text("Boyut")
                    .step_by(1.0),
            );
            ui.horizontal(|ui| {
                for (i, label) in ["Beyaz", "Yeşil", "Sarı", "Camgöbeği", "Turuncu"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(
                        &mut self.crosshair.2,
                        i,
                        RichText::new(*label)
                            .size(CAPTION)
                            .color(CROSSHAIR_COLORS[i]),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                }
            });
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 48.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, WIDGET_RADIUS, BG);
            crosshair(ui.painter(), rect.center(), self.crosshair, 1.0);
            title(ui, "Efektler", BUTTON, ACCENT_2);
            if ui
                .checkbox(&mut self.show_blood, "Kan efektleri")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .changed()
                && !self.show_blood
            {
                self.blood.clear();
                self.local_hit_at = f64::NEG_INFINITY;
            }
            if secondary_button(ui, "Varsayılanlara dön", true) {
                self.sensitivity = 50.0;
                self.hud_scale = 200.0;
                self.show_fps = true;
                self.show_blood = true;
                self.resolution = None;
                self.fps_limit = 60;
                self.crosshair = (0, 8.0, 0);
                self.fullscreen = false;
                self.apply_display();
            }
        });
        if before != self.settings_values() {
            self.save_settings();
        }
    }

    pub(super) fn toast_stack(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|(_, at, _)| self.last_frame - at < 5000.0);
        egui::Area::new(Id::new("toasts"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 20.0))
            .show(ctx, |ui| {
                for (text, at, error) in &self.toasts {
                    let age = ((self.last_frame - at) / 1000.0) as f32;
                    let alpha = (age / 0.2).min(1.0) * ((5.0 - age) / 0.6).clamp(0.0, 1.0);
                    ui.add_space((1.0 - alpha) * 8.0);
                    ui.scope(|ui| {
                        ui.set_opacity(alpha);
                        let response = card(ui, 420.0, |ui| {
                            ui.label(text);
                        });
                        let r = response.response.rect;
                        ui.painter().line_segment(
                            [
                                r.left_top() + vec2(2.0, 10.0),
                                r.left_bottom() - vec2(-2.0, 10.0),
                            ],
                            Stroke::new(3.0, if *error { DANGER } else { SUCCESS }),
                        );
                    });
                }
            });
    }

    pub(super) fn hud(&self, ctx: &egui::Context) {
        if !self.connected
            || !matches!(
                self.screen,
                Screen::Playing
                    | Screen::Paused
                    | Screen::Dead
                    | Screen::Result
                    | Screen::MatchIntro
                    | Screen::Settings
            )
        {
            return;
        }
        let screen = ctx.viewport_rect();
        let scale = self.hud_scale / 100.0;
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("hud")));
        if self.screen == Screen::Playing && self.health > 0 {
            crosshair(&p, screen.center(), self.crosshair, scale);
            if self.last_frame - self.hitmark_at < 150.0 {
                for (a, b) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
                    p.line_segment(
                        [
                            screen.center() + vec2(a, b) * 6.0 * scale,
                            screen.center() + vec2(a, b) * 11.0 * scale,
                        ],
                        Stroke::new(2.0 * scale, TEXT),
                    );
                }
            }
            let cooldown =
                1.0 - ((self.last_frame - self.shot_at) / FIRE_COOLDOWN_MS).clamp(0.0, 1.0) as f32;
            if cooldown > 0.0 {
                let points = (0..=24)
                    .map(|i| {
                        let a = i as f32 / 24.0 * std::f32::consts::TAU * cooldown;
                        screen.center() + vec2(a.cos(), a.sin()) * (self.crosshair.1 + 8.0) * scale
                    })
                    .collect();
                p.add(egui::Shape::line(
                    points,
                    Stroke::new(scale, MUTED.gamma_multiply(0.6)),
                ));
            }
        }
        let origin = pos2(screen.left() + 24.0, screen.bottom() - 24.0 - 10.0 * scale);
        p.text(
            origin - vec2(0.0, 8.0 * scale),
            Align2::LEFT_BOTTOM,
            "CAN",
            FontId::proportional(CAPTION * scale),
            MUTED,
        );
        for i in 0..3 {
            let rect = Rect::from_min_size(
                origin + vec2(i as f32 * 42.0 * scale, 0.0),
                vec2(36.0, 10.0) * scale,
            );
            let color = if i < self.health {
                if self.health == 1 {
                    DANGER
                        .gamma_multiply(0.65 + 0.35 * (self.last_frame as f32 * 0.006).sin().abs())
                } else {
                    ACCENT_2
                }
            } else {
                PANEL_HI
            };
            p.rect_filled(rect, 3.0 * scale, color);
        }
        if self.show_fps {
            p.text(
                screen.right_bottom() - vec2(16.0, 16.0),
                Align2::RIGHT_BOTTOM,
                format!("{} FPS", self.fps),
                FontId::proportional(CAPTION * scale),
                MUTED,
            );
        }
        let panel = Rect::from_center_size(
            pos2(screen.center().x, 16.0 + 32.0 * scale),
            vec2(330.0, 64.0) * scale,
        );
        p.add(SHADOW.as_shape(panel, PANEL_RADIUS));
        p.rect_filled(panel, PANEL_RADIUS, PANEL);
        p.rect_stroke(
            panel,
            PANEL_RADIUS,
            Stroke::new(1.0, BORDER),
            StrokeKind::Inside,
        );
        let center = panel.center();
        let scores = if self.mode == Mode::Ranked && self.team == Some(Team::Girls) {
            [self.score[1], self.score[0]]
        } else {
            self.score
        };
        for (i, dx) in [-104.0, 104.0].into_iter().enumerate() {
            let team = if self.mode == Mode::Ranked {
                if i == 0 {
                    self.team.unwrap_or(Team::Boys)
                } else if self.team == Some(Team::Girls) {
                    Team::Boys
                } else {
                    Team::Girls
                }
            } else if i == 0 {
                Team::Boys
            } else {
                Team::Girls
            };
            let label = if self.mode == Mode::Ranked {
                if i == 0 {
                    self.username.as_str()
                } else {
                    self.opponent.as_ref().map_or("Rakip", |o| o.0.as_str())
                }
            } else {
                team.label()
            };
            p.text(
                center + vec2(dx, -16.0) * scale,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(CAPTION * scale),
                team.color(),
            );
            p.text(
                center + vec2(dx, 10.0) * scale,
                Align2::CENTER_CENTER,
                scores[i].to_string(),
                FontId::proportional(SECTION * scale),
                team.color(),
            );
        }
        p.text(
            center - vec2(0.0, 12.0 * scale),
            Align2::CENTER_CENTER,
            format!("İlk {}", self.kill_target),
            FontId::proportional(CAPTION * scale),
            MUTED,
        );
        if self.mode == Mode::Ranked {
            let left = (self.ranked_ends_at - self.last_frame) / 1000.0;
            p.text(
                center + vec2(0.0, 12.0 * scale),
                Align2::CENTER_CENTER,
                clock(left),
                FontId::proportional(BODY * scale),
                if left < 30.0 { DANGER } else { TEXT },
            );
        }
        egui::Area::new(Id::new("kill-feed"))
            .anchor(Align2::RIGHT_TOP, vec2(-16.0, panel.bottom() + 16.0))
            .show(ctx, |ui| {
                for (killer, victim, kt, vt, at) in &self.kill_feed {
                    let age = self.last_frame - at;
                    if age > 5000.0 {
                        continue;
                    }
                    ui.scope(|ui| {
                        ui.set_opacity(((5000.0 - age) / 800.0).clamp(0.0, 1.0) as f32);
                        egui::Frame::new()
                            .fill(PANEL)
                            .corner_radius(WIDGET_RADIUS)
                            .stroke(Stroke::new(
                                1.0,
                                if killer == &self.username || victim == &self.username {
                                    ACCENT
                                } else {
                                    BORDER
                                },
                            ))
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(killer).size(SMALL * scale).color(kt.color()),
                                    );
                                    ui.label(RichText::new("▸").size(SMALL * scale));
                                    ui.label(
                                        RichText::new(victim).size(SMALL * scale).color(vt.color()),
                                    );
                                });
                            });
                    });
                }
            });
        if self.show_blood && self.last_frame - self.local_hit_at < 350.0 {
            let alpha = (1.0 - ((self.last_frame - self.local_hit_at) / 350.0)) as f32;
            for i in 0..12 {
                let r = screen.shrink(i as f32 * 2.0);
                p.rect_stroke(
                    r,
                    0,
                    Stroke::new(
                        3.0,
                        DANGER.gamma_multiply(alpha * (1.0 - i as f32 / 12.0) * 0.2),
                    ),
                    StrokeKind::Inside,
                );
            }
        }
        if ctx.input(|i| i.key_down(Key::Tab)) {
            self.scoreboard(ctx);
        }
    }

    fn scoreboard(&self, ctx: &egui::Context) {
        let scale = self.hud_scale / 100.0;
        egui::Area::new(Id::new("scoreboard"))
            .order(Order::Tooltip)
            .default_size(vec2(760.0, ctx.viewport_rect().height() - 48.0))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let width = (ctx.viewport_rect().width() - 48.0).min(760.0 * scale);
                card(ui, width, |ui| {
                    title(ui, "SKOR TABLOSU", SECTION * scale, ACCENT_2);
                    let rows = |ui: &mut egui::Ui, team: Option<Team>| {
                        let mut rows: Vec<_> = self
                            .remote
                            .values()
                            .filter(|p| team.is_none_or(|t| p.team == t))
                            .map(|p| (p.name.as_str(), p.kills, p.deaths, p.team))
                            .collect();
                        if team.is_none() || self.team == team {
                            rows.push((
                                &self.username,
                                self.kills,
                                self.deaths,
                                self.team.unwrap_or(Team::Boys),
                            ));
                        }
                        rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
                        ui.label(
                            RichText::new("Oyuncu                  Leş / Ölüm")
                                .size(CAPTION * scale)
                                .color(MUTED),
                        );
                        for (name, k, d, t) in rows {
                            egui::Frame::new()
                                .fill(if name == self.username {
                                    PANEL_HI
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(WIDGET_RADIUS)
                                .inner_margin(8)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(name).size(SMALL * scale).color(
                                            if name == self.username {
                                                ACCENT
                                            } else {
                                                t.color()
                                            },
                                        ));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!("{k} / {d}"))
                                                        .size(SMALL * scale),
                                                );
                                            },
                                        );
                                    });
                                });
                        }
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("score-rows")
                        .max_height(ctx.viewport_rect().height() - 180.0)
                        .show(ui, |ui| {
                            if self.mode == Mode::Ranked {
                                rows(ui, None);
                            } else {
                                ui.columns(2, |cols| {
                                    for (col, team) in
                                        [Team::Boys, Team::Girls].into_iter().enumerate()
                                    {
                                        title(
                                            &mut cols[col],
                                            team.label(),
                                            BUTTON * scale,
                                            team.color(),
                                        );
                                        rows(&mut cols[col], Some(team));
                                    }
                                });
                            }
                        });
                });
            });
    }
}

fn crosshair(
    p: &egui::Painter,
    center: egui::Pos2,
    (style, size, color): (usize, f32, usize),
    scale: f32,
) {
    let color = CROSSHAIR_COLORS[color];
    if style != 1 {
        for direction in [
            vec2(1.0, 0.0),
            vec2(-1.0, 0.0),
            vec2(0.0, 1.0),
            vec2(0.0, -1.0),
        ] {
            let segment = [
                center + direction * 3.0 * scale,
                center + direction * (size + 3.0) * scale,
            ];
            p.line_segment(segment, Stroke::new(4.0 * scale, Color32::BLACK));
            p.line_segment(segment, Stroke::new(2.0 * scale, color));
        }
    }
    if style != 0 {
        p.circle_filled(center, 2.5 * scale, Color32::BLACK);
        p.circle_filled(center, 1.5 * scale, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiers_and_clock_boundaries() {
        for (rating, name) in [
            (899, "Bronz"),
            (900, "Gümüş"),
            (1099, "Gümüş"),
            (1100, "Altın"),
            (1300, "Platin"),
            (1500, "Elmas"),
        ] {
            assert_eq!(tier(rating).0, name);
        }
        assert_eq!(clock(300.0), "05:00");
        assert_eq!(clock(-1.0), "00:00");
    }

    #[test]
    fn cards_keep_vertical_layout_inside_a_horizontal_lobby() {
        let ctx = egui::Context::default();
        apply_theme(&ctx);
        let mut fonts = egui::FontDefinitions::empty();
        fonts.font_data.insert(
            "test".to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/DejaVuSans.ttf"
            ))),
        );
        fonts
            .families
            .insert(egui::FontFamily::Proportional, vec!["test".to_owned()]);
        ctx.set_fonts(fonts);
        for width in [1024.0, 1100.0, 1366.0, 1920.0] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 640.0))),
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal_top(|ui| {
                        let response = card(ui, 420.0, |ui| {
                            let a = ui.label("TAKIM ÖLÜM MAÇI").rect;
                            let b = ui.label("İlk 40 leş").rect;
                            assert!(b.top() > a.bottom());
                            primary_button(ui, "OYNA", true);
                        });
                        assert!(response.response.rect.width() < 425.0);
                    });
                },
            );
            output.textures_delta.clear();
        }
    }
}
