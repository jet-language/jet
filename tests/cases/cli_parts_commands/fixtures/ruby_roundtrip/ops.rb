$counter = 0

def transform(input)
  $counter += 1
  {
    'count' => $counter,
    'nested' => input['nested'],
    'list' => input['list'],
    'scalar' => input['scalar'],
    'nothing' => nil
  }
end

def fail_call(_input)
  raise 'raw secret failure detail'
end

def sleep_call(input)
  sleep 30
  input
end
