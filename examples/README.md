# Demo resources

A few small FiveM and RedM resources to try OpenIDEA-CfxLUA: open the `examples` folder in OpenIDEA.

| Resource | Shows |
|---|---|
| `[demo]/shop` | Client / server / shared scripts, natives, events between client and server, state bags, a NUI page (`RegisterNUICallback` ↔ `fetch`, `SendNUIMessage` ↔ `switch (action)`), a per-frame loop. |
| `[demo]/garage` | Naming conventions (`cl_*.lua`, `sv_*.lua`), entity state bags, magic values (`SetVehicleColours`), Asset Escrow (`escrow_ignore`). |
| `[redm]/stables` | A RedM resource: `game 'rdr3'`, RedM natives, a backtick hash literal. |

Things to try: type `GetEntity` in a client script, `TriggerServerEvent('` in `shop/client/main.lua`,
`LocalPlayer.state.` in a client script, or open the **CfxLua** tool window on the left.
