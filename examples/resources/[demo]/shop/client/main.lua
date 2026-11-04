local bIsShopOpen = false

local function SetShopOpen(bOpen)
    bIsShopOpen = bOpen
    SetNuiFocus(bOpen, bOpen)
    SendNUIMessage({ action = bOpen and 'open' or 'close', items = Config.tItems })
end

CreateThread(function()
    while true do
        local vPosition = GetEntityCoords(PlayerPedId())
        local fDistance = #(vPosition - Config.vShopPosition)

        if (fDistance < Config.fInteractDistance and IsControlJustPressed(0, 38)) then
            SetShopOpen(true)
        end

        Wait(0)
    end
end)

RegisterNUICallback('buy', function(tData, cb)
    TriggerServerEvent('shop:buy', tData.item)
    cb('ok')
end)

RegisterNUICallback('close', function(_, cb)
    SetShopOpen(false)
    cb('ok')
end)

RegisterNetEvent('shop:bought', function(sItem)
    local iMoney = LocalPlayer.state.money
    print(('Bought %s, %d left'):format(sItem, iMoney))
end)
