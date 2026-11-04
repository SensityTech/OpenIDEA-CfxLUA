-- Server script

AddEventHandler('onResourceStart', function(resourceName)
    if resourceName ~= GetCurrentResourceName() then
        return
    end
    print(('[%s] started'):format(resourceName))
end)
