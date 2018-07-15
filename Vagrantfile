# For a complete reference, please see the online documentation at
# https://docs.vagrantup.com.

require 'yaml'
dir = File.dirname(File.expand_path(__FILE__))

# defaults
local_settings = YAML::load_file("#{dir}/vagrant/local_settings.yml.example")

if File.exist?("#{dir}/vagrant/local_settings.yml")
  local_settings.merge!(YAML::load_file("#{dir}/vagrant/local_settings.yml"))
end

Vagrant.configure("2") do |config|

  config.vm.define "osx" do |osx|
    osx.vm.box = "AndrewDryga/vagrant-box-osx"
  end

  config.vm.define "linux" do |linux|
    linux.vm.box = "ubuntu/bionic64"
    linux.vm.provision "install_dependencies", type: "shell", path: "vagrant/linux/install_dependencies.sh", privileged: false
    linux.vm.provision "moqt", type: "shell", path: "vagrant/linux/moqt.sh", privileged: false

    if local_settings["moqt_workspace_path"]
      linux.vm.synced_folder local_settings["moqt_workspace_path"], "/home/vagrant/moqt_workspace"
    else
      puts "local_settings[\"moqt_workspace_path\"] was not set."
    end
  end
end
