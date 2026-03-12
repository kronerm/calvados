use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
};

/// `Html.FROM_HTML_MODE_COMPACT`.
const FROM_HTML_MODE_COMPACT: i32 = 0;

/// `android.text.Html.fromHtml(source, FROM_HTML_MODE_COMPACT)`: styled text
/// (`<b>` and the like) as a `Spanned`, a `CharSequence`.
pub fn from_html<'env_scope>(
    env: &mut Env<'env_scope>,
    source: &str,
) -> anyhow::Result<JObject<'env_scope>> {
    let source = JString::new(env, source)?;
    let class = env.find_class(jni_str!("android/text/Html"))?;
    Ok(env
        .call_static_method(
            &class,
            jni_str!("fromHtml"),
            jni_sig!("(Ljava/lang/String;I)Landroid/text/Spanned;"),
            &[JValue::Object(&source), JValue::Int(FROM_HTML_MODE_COMPACT)],
        )?
        .l()?)
}
