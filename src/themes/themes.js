export const THEME_STORAGE_KEY = 'log73.theme';

export const THEME_OPTIONS = [
  { id: 'default', label: 'Default' },
  { id: 'modern-dark-radio', label: 'Modern Dark Radio' },
  { id: 'classic-terminal', label: 'Classic Terminal' },
  { id: 'clean-light-desktop', label: 'Clean Light Desktop' },
  { id: 'n1mm-contest', label: 'N1MM-ish Contest' },
  { id: 'high-contrast', label: 'High Contrast' },
];

export const THEME_CLASS_NAMES = THEME_OPTIONS.filter(
  (theme) => theme.id !== 'default',
).map((theme) => `theme-${theme.id}`);

export function normalizeTheme(theme) {
  return THEME_OPTIONS.some((option) => option.id === theme)
    ? theme
    : 'default';
}

export function loadTheme() {
  return normalizeTheme(localStorage.getItem(THEME_STORAGE_KEY) ?? 'default');
}

export function themeClassName(theme) {
  const normalizedTheme = normalizeTheme(theme);
  return normalizedTheme === 'default' ? null : `theme-${normalizedTheme}`;
}
