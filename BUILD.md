# Evolution Simulator — Build Instructions

## Prerequisites (one-time)
```
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

## Build
```
cd sim
wasm-pack build --target web --release
copy pkg\* ..\web\pkg\   (Windows)
cp -r pkg/* ../web/pkg/   (Mac/Linux)
```

Or on Windows PowerShell:
```
cd sim
wasm-pack build --target web --release
xcopy pkg ..\web\pkg /E /I /Y
```

## Run
```
cd web
python -m http.server 8080
```
Then open: http://localhost:8080

## Controls
| Key / Control | Effect |
|---------------|--------|
| Space / P | Pause / resume |
| . (period) | Step one tick (only while paused) |
| Step button | Step one tick (only while paused) |
| F key | Toggle nutrient overlay |
| Water level slider | Raise/lower sea level |
| Nutrient flow rate | How fast nutrients run downhill |
| Hillshade strength | Shadow depth on slopes |
| New terrain | New random seed (not reproducible — note the seed shown in HUD) |
| Seed input + Load | Load a specific reproducible seed (Enter also works) |
| ↺ Restart / R key | Re-run the current seed — same terrain, fresh population |
| 🌍 Main World | Full default world preset (4000×2000) |
| 🔬 Dev World | Small flat test world preset (800×400, 2:1) |
