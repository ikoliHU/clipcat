// Fordítások: a szövegek a Rust oldaltól jönnek (ui/locales/<nyelv>.json), így mindkét oldal ugyanazt használja.
//   data-i18n="kulcs"        -> textContent
//   data-i18n-title="kulcs"  -> title attribútum

const i18n = { lang: "hu", messages: {} };

// A kulcshoz tartozó szöveg a {név} helyőrzők kitöltésével; hiányzó kulcsnál maga a kulcs
function t(key, params) {
  const text = i18n.messages[key] ?? key;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
}

function applyTranslations(root = document) {
  root.querySelectorAll("[data-i18n]").forEach((el) => (el.textContent = t(el.dataset.i18n)));
  root.querySelectorAll("[data-i18n-title]").forEach((el) => (el.title = t(el.dataset.i18nTitle)));
}

async function loadLocale() {
  const { lang, messages } = await window.__TAURI__.core.invoke("get_locale");
  i18n.lang = lang;
  i18n.messages = messages;
  document.documentElement.lang = lang;
  applyTranslations();
}
