#!/bin/bash

set -e

RUST_APP_NAME=mrial_player
MACOS_BIN_NAME=mrial_player
MACOS_RUST_APP_NAME=Mrial
MACOS_APP_DIR=$MACOS_RUST_APP_NAME.app

mkdir -p dist
cd dist
echo "Creating app directory structure"
rm -rf $MACOS_RUST_APP_NAME
rm -rf $MACOS_APP_DIR
mkdir -p $MACOS_APP_DIR/Contents/MacOS

echo "Copying binary"
MACOS_APP_BIN=$MACOS_APP_DIR/Contents/MacOS/$MACOS_BIN_NAME
cp ../target/release/$RUST_APP_NAME $MACOS_APP_BIN

echo "Copying launcher"
cp ../macos/scripts/launch.sh $MACOS_APP_DIR/Contents/MacOS/$MACOS_RUST_APP_NAME

echo "Copying Icon"
mkdir -p $MACOS_APP_DIR/Contents/Resources
cp ../macos/Info.plist $MACOS_APP_DIR/Contents/
# cp ../macos/logo.icns $MACOS_APP_DIR/Contents/Resources/

echo "Creating dmg"
mkdir -p $MACOS_RUST_APP_NAME
cp -r $MACOS_APP_DIR $MACOS_RUST_APP_NAME/
rm -rf $MACOS_RUST_APP_NAME/.Trashes

FULL_NAME=$MACOS_RUST_APP_NAME

create-dmg --window-size 500 500 --app-drop-link 370 200 --icon-size 125 --icon $FULL_NAME.app 120 200 $FULL_NAME.dmg $MACOS_RUST_APP_NAME
rm -rf $MACOS_RUST_APP_NAME