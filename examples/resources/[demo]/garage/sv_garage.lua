RegisterNetEvent('garage:spawned', function(iNetId)
    local iVehicle = NetworkGetEntityFromNetworkId(iNetId)
    Entity(iVehicle).state:set('owner', GetPlayerName(source), true)
end)
