export const THEME_STORAGE_KEY = 'log73.theme';
export const ZOOM_STORAGE_KEY = 'log73.zoom';

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

export const ZOOM_OPTIONS = [
  { value: 1, label: '100%' },
  { value: 1.25, label: '125%' },
  { value: 1.5, label: '150%' },
  { value: 2, label: '200%' },
];

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

export function normalizeZoom(zoom) {
  const parsedZoom = Number(zoom);
  return ZOOM_OPTIONS.some((option) => option.value === parsedZoom)
    ? parsedZoom
    : 1;
}

export function loadZoom() {
  return normalizeZoom(localStorage.getItem(ZOOM_STORAGE_KEY) ?? 1);
}
