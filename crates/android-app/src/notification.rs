//! The persistent notification, the app's only UI.

use jni::JavaVM;
use jni::objects::JObject;

use crate::intents::{self, ACTION_AUTO_CONNECT, ACTION_AUTO_SWITCH, ACTION_NOISE_PREFIX};
use crate::jni::android::app::activity_thread::ActivityThread;
use crate::jni::android::app::notification::{
    DecoratedCustomViewStyle, Importance, Notification, NotificationChannel,
};
use crate::jni::android::app::pending_intent::{Flags, PendingIntent};
use crate::jni::android::app::service::Service;
use crate::jni::android::content::context::Context;
use crate::jni::android::content::res::Resources;
use crate::jni::android::text::html;
use crate::jni::android::widget::remote_views::{RemoteViews, Visibility};
use crate::proto::aacp::NoiseControlMode;
use crate::proto::display::{self, Buds, Component, Place};
use crate::proto::engine::Engine;
use crate::service::Conn;

pub(crate) const CHANNEL_ID: &str = "calvados_persistent";

pub(crate) const NOTIFICATION_ID: i32 = 1;

/// `Notification.FOREGROUND_SERVICE_IMMEDIATE`
pub(crate) const FOREGROUND_SERVICE_IMMEDIATE: i32 = 1;

/// Create the notification channel and enter the foreground with a
/// placeholder; the loop replaces it within one tick.
pub(crate) fn start_foreground<'local>(
    env: &mut jni::Env<'local>,
    service: &Service<JObject<'local>>,
) -> anyhow::Result<()> {
    let context = service.as_context();
    let channel = NotificationChannel::new(env, CHANNEL_ID, "Calvados", Importance::Low)?
        .set_show_badge(env, false)?;
    context
        .notification_manager(env)?
        .create_notification_channel(env, &channel)?;
    let notification = Content::starting().build(env, &context)?;
    service.start_foreground(env, NOTIFICATION_ID, &notification)
}

/// What the notification shows; posted only when it changes. Laid out like
/// the Linux panel (`res/layout`): collapsed, the name, status and levels;
/// expanded, the batteries, the noise mode as tiles and the two settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Content {
    /// `AirPods Pro · Active`.
    title: String,
    /// The connection state, or a warning; empty for none.
    text: String,
    /// While connected.
    buds: Option<Buds>,
    /// [`display::mode_name`]: empty while unknown.
    noise: &'static str,
    auto_switch: bool,
    auto_connect: bool,
}

/// The tiles: mode, view id.
const MODES: [(NoiseControlMode, &str); 3] = [
    (NoiseControlMode::NoiseCancellation, "tile_anc"),
    (NoiseControlMode::Transparency, "tile_transparency"),
    (NoiseControlMode::Adaptive, "tile_adaptive"),
];

impl Content {
    pub(crate) fn of(engine: &Engine, conn: &Conn) -> Self {
        let state = engine.state();
        let settings = engine.settings();
        let name = state.name.as_deref().unwrap_or("AirPods");
        let connected = matches!(conn, Conn::Connected { .. });
        let text = match conn {
            Conn::Disconnected => "Not connected",
            Conn::Connecting { .. } => "Connecting…",
            Conn::Connected { .. } if engine.local_mac().is_none() => {
                "No handoff: this phone's MAC is unknown"
            }
            Conn::Connected { .. } => "",
        };
        Self {
            title: match connected {
                true => format!("{name} · {}", display::ownership_label(engine.ownership())),
                false => name.to_owned(),
            },
            text: text.to_owned(),
            buds: connected.then(|| Buds::of(state)),
            noise: display::mode_name(state.noise_mode),
            auto_switch: settings.auto_switch,
            auto_connect: settings.auto_connect,
        }
    }

    /// The placeholder `startForeground` needs before the loop runs.
    fn starting() -> Self {
        Self {
            title: "Calvados".into(),
            text: "Starting…".into(),
            buds: None,
            noise: "",
            auto_switch: true,
            auto_connect: true,
        }
    }

    pub(crate) fn post(&self, jvm: &JavaVM) {
        let posted = jvm.attach_current_thread(|env| {
            let context = ActivityThread::current_activity_thread(env)?.get_application(env)?;
            let notification = self.build(env, &context)?;
            context
                .notification_manager(env)?
                .notify(env, NOTIFICATION_ID, &notification)
        });
        if let Err(e) = posted {
            tracing::warn!("Failed to update notification: {e:#}");
        }
    }

    /// `Noise control`, saying so when the mode is Off (never offered, but
    /// the pods can report it) or not known yet.
    fn noise_heading(&self) -> &'static str {
        match (self.buds.is_some(), self.noise) {
            (true, "off") => "Noise control · Off",
            (true, "") => "Noise control · unknown yet",
            _ => "Noise control",
        }
    }

    fn build<'local>(
        &self,
        env: &mut jni::Env<'local>,
        context: &Context<impl jni::refs::Reference + AsRef<JObject<'local>>>,
    ) -> anyhow::Result<Notification<JObject<'local>>> {
        let res = Res {
            package: context.get_package_name(env)?,
            resources: context.get_resources(env)?,
        };
        let collapsed = self.collapsed(env, &res)?;
        let expanded = self.expanded(env, context, &res)?;
        let style = DecoratedCustomViewStyle::new(env)?;
        let icon = res.get(env, "ic_stat_airpods", "drawable")?;
        // Title and text also serve the lock screen's and accessibility's
        // plain rendering.
        Notification::builder(env, context.as_obj(), CHANNEL_ID)?
            .set_content_title(env, &self.title)?
            .set_content_text(env, &self.text)?
            .set_small_icon(env, icon)?
            .set_show_when(env, false)?
            .set_ongoing(env, true)?
            .set_foreground_service_behavior(env, FOREGROUND_SERVICE_IMMEDIATE)?
            .set_style(env, &style)?
            .set_custom_content_view(env, &collapsed)?
            .set_custom_big_content_view(env, &expanded)?
            .build(env)
    }

    /// Name and status, then the levels like the Linux bar: the buds left
    /// then right, the primary's in bold, the case's after its icon.
    fn collapsed<'local>(
        &self,
        env: &mut jni::Env<'local>,
        res: &Res<'local>,
    ) -> anyhow::Result<RemoteViews<JObject<'local>>> {
        let layout = res.get(env, "notification_collapsed", "layout")?;
        let views = RemoteViews::new(env, &res.package, layout)?;
        let ui = Ui { res, views: &views };
        ui.text(env, "title", &self.title)?;
        ui.optional_text(env, "text", &self.text)?;
        let buds = self.buds.unwrap_or_default();
        let levels = [
            ("sum_left", buds.left),
            ("sum_right", buds.right),
            ("sum_case", buds.case),
        ]
        .map(|(prefix, component)| (prefix, component.filter(|c| !c.last_known)));
        ui.show(env, "levels", levels.iter().any(|(_, c)| c.is_some()))?;
        ui.show(env, "sum_case_icon", levels[2].1.is_some())?;
        for (prefix, component) in levels {
            let level = format!("{prefix}_level");
            ui.show(env, &level, component.is_some())?;
            ui.show(
                env,
                &format!("{prefix}_charging"),
                component.is_some_and(|c| c.charging),
            )?;
            if let Some(component) = component {
                let text = match component.primary {
                    true => format!("<b>{}%</b>", component.level),
                    false => format!("{}%", component.level),
                };
                ui.html(env, &level, &text)?;
            }
        }
        Ok(views)
    }

    /// Batteries, noise tiles and settings, like the Linux panel.
    fn expanded<'local>(
        &self,
        env: &mut jni::Env<'local>,
        context: &Context<impl jni::refs::Reference + AsRef<JObject<'local>>>,
        res: &Res<'local>,
    ) -> anyhow::Result<RemoteViews<JObject<'local>>> {
        let layout = res.get(env, "notification_expanded", "layout")?;
        let views = RemoteViews::new(env, &res.package, layout)?;
        let ui = Ui { res, views: &views };
        ui.text(env, "title", &self.title)?;
        ui.optional_text(env, "text", &self.text)?;
        ui.show(env, "batteries", self.buds.is_some())?;
        let buds = self.buds.unwrap_or_default();
        for (prefix, component) in [
            ("battery_left", buds.left),
            ("battery_right", buds.right),
            ("battery_case", buds.case),
        ] {
            ui.battery(env, prefix, component)?;
        }
        ui.text(env, "noise_heading", self.noise_heading())?;
        let connected = self.buds.is_some();
        for (request_code, (mode, tile)) in (0..).zip(MODES) {
            let selected = self.noise == display::mode_name(Some(mode));
            let background = match selected {
                true => "tile_selected",
                false => "tile",
            };
            let background = res.get(env, background, "drawable")?;
            let id = ui.id(env, tile)?;
            views.set_int(env, id, "setBackgroundResource", background)?;
            views.set_float(env, id, "setAlpha", if connected { 1.0 } else { 0.5 })?;
            let action = format!("{ACTION_NOISE_PREFIX}{}", display::mode_name(Some(mode)));
            ui.on_click(env, context, tile, request_code, &action)?;
        }
        for (request_code, (row, enabled, action)) in (10..).zip([
            ("setting_switch", self.auto_switch, ACTION_AUTO_SWITCH),
            ("setting_connect", self.auto_connect, ACTION_AUTO_CONNECT),
        ]) {
            let toggle = match enabled {
                true => "ic_toggle_right",
                false => "ic_toggle_left",
            };
            ui.image(env, &format!("{row}_toggle"), toggle)?;
            ui.on_click(env, context, row, request_code, action)?;
        }
        Ok(views)
    }
}

/// The app's resources by name: there's no generated `R` class.
struct Res<'local> {
    package: String,
    resources: Resources<JObject<'local>>,
}

impl<'local> Res<'local> {
    fn get(&self, env: &mut jni::Env<'local>, name: &str, kind: &str) -> anyhow::Result<i32> {
        match self
            .resources
            .get_identifier(env, name, kind, &self.package)?
        {
            0 => anyhow::bail!("no {kind} resource {name}"),
            id => Ok(id),
        }
    }
}

/// Setters on one layout's views, by id name.
struct Ui<'a, 'local> {
    res: &'a Res<'local>,
    views: &'a RemoteViews<JObject<'local>>,
}

impl<'local> Ui<'_, 'local> {
    fn id(&self, env: &mut jni::Env<'local>, name: &str) -> anyhow::Result<i32> {
        self.res.get(env, name, "id")
    }

    fn show(&self, env: &mut jni::Env<'local>, id: &str, shown: bool) -> anyhow::Result<()> {
        let id = self.id(env, id)?;
        self.views.set_view_visibility(
            env,
            id,
            if shown {
                Visibility::Visible
            } else {
                Visibility::Gone
            },
        )
    }

    fn text(&self, env: &mut jni::Env<'local>, id: &str, text: &str) -> anyhow::Result<()> {
        let id = self.id(env, id)?;
        let text = jni::objects::JString::new(env, text)?;
        self.views.set_text_view_text(env, id, &text)
    }

    /// `text`, or the view hidden when it's empty.
    fn optional_text(
        &self,
        env: &mut jni::Env<'local>,
        id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        self.show(env, id, !text.is_empty())?;
        self.text(env, id, text)
    }

    fn html(&self, env: &mut jni::Env<'local>, id: &str, source: &str) -> anyhow::Result<()> {
        let id = self.id(env, id)?;
        let text = html::from_html(env, source)?;
        self.views.set_text_view_text(env, id, &text)
    }

    fn image(&self, env: &mut jni::Env<'local>, id: &str, drawable: &str) -> anyhow::Result<()> {
        let id = self.id(env, id)?;
        let drawable = self.res.get(env, drawable, "drawable")?;
        self.views.set_image_view_resource(env, id, drawable)
    }

    /// A battery column: level, its glyph (dimmed when last known), the
    /// bud's place, and a dot by the primary bud.
    fn battery(
        &self,
        env: &mut jni::Env<'local>,
        prefix: &str,
        component: Option<Component>,
    ) -> anyhow::Result<()> {
        let level = format!("{prefix}_level");
        let battery = format!("{prefix}_battery");
        let place = format!("{prefix}_place");
        self.show(
            env,
            &format!("{prefix}_primary"),
            component.is_some_and(|c| c.primary),
        )?;
        let Some(component) = component else {
            self.text(env, &level, "—")?;
            self.show(env, &battery, false)?;
            return self.show(env, &place, false);
        };
        let text = match component.last_known {
            true => format!("({}%)", component.level),
            false => format!("{}%", component.level),
        };
        self.text(env, &level, &text)?;
        self.show(env, &battery, true)?;
        self.image(env, &battery, battery_glyph(component))?;
        let alpha = if component.last_known { 128 } else { 255 };
        let id = self.id(env, &battery)?;
        self.views.set_int(env, id, "setImageAlpha", alpha)?;
        let glyph = match component.place {
            Some(Place::InEar) => Some("ic_ear"),
            Some(Place::InCase) => Some("ic_device_airpods_case"),
            None => None,
        };
        self.show(env, &place, glyph.is_some())?;
        match glyph {
            Some(glyph) => self.image(env, &place, glyph),
            None => Ok(()),
        }
    }

    fn on_click(
        &self,
        env: &mut jni::Env<'local>,
        context: &Context<impl jni::refs::Reference + AsRef<JObject<'local>>>,
        id: &str,
        request_code: i32,
        action: &str,
    ) -> anyhow::Result<()> {
        let id = self.id(env, id)?;
        let intent = intents::service_intent(env, context, action)?;
        let flags = Flags::IMMUTABLE | Flags::UPDATE_CURRENT;
        let pending =
            PendingIntent::get_foreground_service(env, context, request_code, &intent, flags)?;
        self.views.set_on_click_pending_intent(env, id, &pending)
    }
}

/// The Tabler battery glyph for a level, as the Linux panel picks it.
fn battery_glyph(component: Component) -> &'static str {
    match component.level {
        _ if component.charging => "ic_battery_charging",
        88.. => "ic_battery_4",
        63.. => "ic_battery_3",
        38.. => "ic_battery_2",
        13.. => "ic_battery_1",
        _ => "ic_battery",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_glyph_follows_the_level() {
        let at = |level, charging| Component {
            level,
            charging,
            last_known: false,
            place: None,
            primary: false,
        };
        assert_eq!(battery_glyph(at(90, false)), "ic_battery_4");
        assert_eq!(battery_glyph(at(12, false)), "ic_battery");
        assert_eq!(battery_glyph(at(12, true)), "ic_battery_charging");
    }
}
