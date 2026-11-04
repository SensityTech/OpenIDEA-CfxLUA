fx_version 'cerulean'
game 'gta5'
lua54 'yes'

author 'OpenIDEA-CfxLUA demo'
description 'A small shop: client, server and shared scripts with a NUI page'
version '1.0.0'

shared_script 'shared/config.lua'
client_scripts { 'client/*.lua' }
server_scripts { 'server/*.lua' }

ui_page 'web/index.html'

files {
    'web/index.html',
    'web/app.js',
}
