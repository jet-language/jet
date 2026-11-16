require "json"

Person = Struct.new(:id, :name, :dept, :salary, :active, :tags, keyword_init: true)
people = JSON.parse(File.read("w06.json"), symbolize_names: true).map { |item| Person.new(**item) }
active = people.select(&:active)
puts "records=#{people.length} active=#{active.length} salary=#{active.sum(&:salary)}"
