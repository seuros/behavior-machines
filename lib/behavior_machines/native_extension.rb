# frozen_string_literal: true

require 'rbconfig'

module BehaviorMachines
  # Loads the optional native backend. It is opt-in: set
  # BEHAVIOR_MACHINES_NATIVE=1 before requiring behavior_machines.
  module NativeExtension
    class << self
      # Why the last load attempt failed, if it did.
      attr_reader :error

      def load!
        return @loaded if defined?(@loaded)
        return @loaded = false unless ENV['BEHAVIOR_MACHINES_NATIVE'] == '1'

        errors = []
        candidates.each do |path|
          require path
          return @loaded = true
        rescue LoadError => e
          errors << "#{path}: #{e.message}"
        end
        @error = errors.join(' | ')
        @loaded = false
      end

      def loaded? = load!

      private

      # Prefer a build matching this Ruby version and platform.
      def candidates
        dlext = RbConfig::CONFIG['DLEXT']
        root = File.expand_path('..', __dir__)
        builds = Dir.glob(File.join(root, 'behavior_machines_native', '**', "behavior_machines_native.#{dlext}"))
        ranked = builds.sort_by do |path|
          score = 0
          score -= 2 if path.include?(RbConfig::CONFIG['ruby_version'])
          score -= 1 if path.include?(RbConfig::CONFIG['arch'])
          [score, path]
        end
        paths = ranked.map { |path| path.delete_prefix("#{root}/").delete_suffix(".#{dlext}") }
        (paths + ['behavior_machines_native/behavior_machines_native']).uniq
      end
    end
  end
end
