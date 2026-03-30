module.exports = {
  content: ["./src/**/*.rs", "./index.html"],
  theme: {
    extend: {
      boxShadow: {
        shell: "0 18px 60px rgba(31, 41, 51, 0.12)",
      },
      colors: {
        sand: {
          50: "#f8f4ec",
          100: "#efe8dc",
          200: "#f3efe7",
        },
        ink: {
          DEFAULT: "#1f2933",
          muted: "#59636e",
        },
        accent: {
          DEFAULT: "#006d77",
          strong: "#004e57",
          warm: "#ee9b00",
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
