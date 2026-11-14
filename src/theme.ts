// Apparence claire ou sombre : préférence propre à ce poste, jamais partagée ni synchronisée.
export type Theme = 'light' | 'dark' | 'auto';
const KEY = 'libre-erp-theme';
const system = () => window.matchMedia('(prefers-color-scheme: dark)');

export function storedTheme(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    return value === 'dark' || value === 'auto' ? value : 'light';
  } catch {
    return 'light';
  }
}
export function applyTheme(theme: Theme) {
  const dark = theme === 'dark' || (theme === 'auto' && system().matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
}
export function saveTheme(theme: Theme) {
  try { localStorage.setItem(KEY, theme); } catch { /* préférence non mémorisée */ }
  applyTheme(theme);
}
export function initTheme() {
  applyTheme(storedTheme());
  system().addEventListener('change', () => applyTheme(storedTheme()));
}
