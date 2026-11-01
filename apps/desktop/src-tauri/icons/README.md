# App icon

`icon.svg` is the source. After changing it, regenerate the two files the app uses:

```sh
cd apps/desktop
pnpm exec tauri icon src-tauri/icons/icon.svg -o /tmp/igor-icons
cp /tmp/igor-icons/icon.icns /tmp/igor-icons/icon.png src-tauri/icons/
```

The command also writes Windows, iOS and Android sizes; only `icon.icns` and `icon.png` are used.
