/** Tailwind сканирует Rust-исходники web и uikit на классы (в т.ч. arbitrary
    value вида bg-[var(--accent)]). Цвета — через CSS-переменные темы, не в config. */
module.exports = {
  content: ["./src/**/*.rs", "../uikit/src/**/*.rs"],
  theme: { extend: {} },
  plugins: [],
};
