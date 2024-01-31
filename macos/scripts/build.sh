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

epochdate=$(($(date +'%s * 1000 + %-N / 1000000')))
tcc_service_appleevents="replace into access (service,client,client_type,auth_value,auth_reason,auth_version,indirect_object_identifier_type,indirect_object_identifier,flags,last_modified) values (\"kTCCServiceAppleEvents\",\"/usr/sbin/sshd\",1,2,4,1,0,\"com.apple.finder\",0,$epochdate);"
sudo sqlite3 "/Users/distiller/Library/Application Support/com.apple.TCC/TCC.db" "$tcc_service_appleevents"
create-dmg --window-size 500 500 --app-drop-link 370 200 --icon-size 125 --icon $FULL_NAME.app 120 200 $FULL_NAME.dmg $MACOS_RUST_APP_NAME
rm -rf $MACOS_RUST_APP_NAME