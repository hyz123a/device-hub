module.exports = {
  content: ["./src/**/*.rs", "./index.html"],
  theme: {
    extend: {
      boxShadow: {
        shell: "var(--dh-shadow-shell)",
      },
      colors: {
        sand: {
          50: "rgb(var(--dh-page-soft) / <alpha-value>)",
          100: "rgb(var(--dh-page) / <alpha-value>)",
          200: "rgb(var(--dh-surface-muted) / <alpha-value>)",
        },
        ink: {
          DEFAULT: "rgb(var(--dh-text) / <alpha-value>)",
          muted: "rgb(var(--dh-text-muted) / <alpha-value>)",
        },
        accent: {
          DEFAULT: "rgb(var(--dh-accent) / <alpha-value>)",
          strong: "rgb(var(--dh-accent-strong) / <alpha-value>)",
          warm: "rgb(var(--dh-accent-warm) / <alpha-value>)",
        },
      },
      fontFamily: {
        sans: ["IBM Plex Sans", "Segoe UI", "sans-serif"],
        mono: ["IBM Plex Mono", "SFMono-Regular", "monospace"],
      },
    },
  },
  plugins: [],
};
