local function FindItem(sName)
    for _, tItem in ipairs(Config.tItems) do
        if (tItem.sName == sName) then return tItem end
    end
end

RegisterNetEvent('shop:buy', function(sName)
    local iSource = source
    local tItem = FindItem(sName)
    if (not tItem) then return end

    local tState = Player(iSource).state
    local iMoney = tState.money or 0
    if (iMoney < tItem.iPrice) then return end

    tState:set('money', iMoney - tItem.iPrice, true)
    TriggerClientEvent('shop:bought', iSource, sName)
end)

AddEventHandler('playerJoining', function()
    Player(source).state:set('money', 100, true)
end)

GlobalState.shopOpen = true
