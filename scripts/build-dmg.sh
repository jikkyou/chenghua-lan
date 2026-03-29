#!/bin/bash

# 成华县过县 - 构建并打包 DMG 脚本
# 用法: ./scripts/build-dmg.sh

set -e

cd "$(dirname "$0")/.."

# 读取当前版本
CURRENT_VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
echo "当前版本: $CURRENT_VERSION"

# 递增版本号 (0.1.0 -> 0.1.1)
IFS='.' read -ra VERSION_PARTS <<< "$CURRENT_VERSION"
MAJOR=${VERSION_PARTS[0]}
MINOR=${VERSION_PARTS[1]}
PATCH=${VERSION_PARTS[2]}
NEW_PATCH=$((PATCH + 1))
NEW_VERSION="${MAJOR}.${MINOR}.${NEW_PATCH}"
echo "新版本: $NEW_VERSION"

# 更新版本号
node -e "
const fs = require('fs');
const config = JSON.parse(fs.readFileSync('./src-tauri/tauri.conf.json', 'utf8'));
config.version = '$NEW_VERSION';
fs.writeFileSync('./src-tauri/tauri.conf.json', JSON.stringify(config, null, 2));
"
echo "已更新版本号到 $NEW_VERSION"

# 构建
echo "开始构建..."
npm run tauri build

# 生成 DMG
DMG_PATH="./src-tauri/target/release/bundle/dmg/成华县过县_${NEW_VERSION}_aarch64.dmg"
echo "生成 DMG: $DMG_PATH"

# 删除旧的 DMG (如果存在)
rm -f "./src-tauri/target/release/bundle/dmg/成华县过县_"*.dmg

# 创建 DMG
hdiutil create -volname "成华县过县" -srcfolder "./src-tauri/target/release/bundle/macos/成华县过县.app" -ov -format UDZO "$DMG_PATH"

# 删除旧版应用 (如果存在)
OLD_APP="/Applications/成华县过县.app"
if [ -d "$OLD_APP" ]; then
    echo "删除旧版应用: $OLD_APP"
    rm -rf "$OLD_APP"
fi

# 删除旧版 from Dock (如果还在运行)
echo "关闭旧版应用 (如果正在运行)..."
pkill -f "成华县过县" 2>/dev/null || true
sleep 1

echo ""
echo "✅ 构建完成!"
echo "DMG 路径: $(pwd)/$DMG_PATH"
echo "版本: $NEW_VERSION"

# 打开 DMG 所在文件夹
open "./src-tauri/target/release/bundle/dmg/"
