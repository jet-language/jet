$script:Counter = 0

function Get-Stateful {
  param($InputObject)
  $script:Counter += 1
  [ordered]@{
    count = $script:Counter
    nested = $InputObject.nested
    list = @($InputObject.list)
    scalar = $InputObject.scalar
    nothing = $null
  }
}

function Fail { param($InputObject) throw 'raw secret failure detail' }
function Sleep { param($InputObject) Start-Sleep -Seconds 30; return $InputObject }
