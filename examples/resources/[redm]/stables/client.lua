RegisterCommand('horse', function()
    local iPed = PlayerPedId()
    local vPosition = GetEntityCoords(iPed)
    local iModel = `a_c_horse_arabian_white`
    RequestModel(iModel)
    while (not HasModelLoaded(iModel)) do
        Wait(10)
    end
    CreatePed(iModel, vPosition.x + 2.0, vPosition.y, vPosition.z, 0.0, true, false, false, false)
end, false)
