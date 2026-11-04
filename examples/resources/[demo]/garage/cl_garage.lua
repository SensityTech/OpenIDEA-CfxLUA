RegisterCommand('car', function(_, tArgs)
    local iModel = GetHashKey(tArgs[1] or 'adder')
    RequestModel(iModel)
    while (not HasModelLoaded(iModel)) do
        Wait(10)
    end

    local vPosition = GetEntityCoords(PlayerPedId())
    local iVehicle = CreateVehicle(iModel, vPosition.x, vPosition.y, vPosition.z, 0.0, true, false)
    SetVehicleColours(iVehicle, 27, 27)
    TriggerServerEvent('garage:spawned', NetworkGetNetworkIdFromEntity(iVehicle))
end, false)
