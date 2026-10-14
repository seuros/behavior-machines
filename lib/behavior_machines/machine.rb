# frozen_string_literal: true

module BehaviorMachines
  # Leaves that drive objects powered by the state_machines gem, through the
  # helpers it generates for every machine: `<machine>_name` and
  # `fire_<machine>_event`.
  #
  # Each leaf finds its object in the tree context with `access`: a Symbol
  # naming a method on the context, or anything responding to call(ctx).
  module Machine
    def self.accessor(access)
      case access
      when Symbol, String then ->(ctx) { ctx.public_send(access) }
      else
        raise ArgumentError, "access must be a Symbol or respond to call, got #{access.inspect}" unless access.respond_to?(:call)

        access
      end
    end

    # Succeeds while the machine is in one of `states`, fails otherwise.
    class InState
      def initialize(access, *states, machine: :state)
        raise ArgumentError, 'InState needs at least one state' if states.empty?

        @access = Machine.accessor(access)
        @states = states.map(&:to_sym).freeze
        @reader = :"#{machine}_name"
      end

      def tick(ctx, _now) = @states.include?(@access.call(ctx).public_send(@reader)) ? :success : :failure
    end

    # Fires `event` each tick: succeeds if the machine accepts it, fails if it
    # is rejected.
    class Fire
      def initialize(access, event, machine: :state)
        @access = Machine.accessor(access)
        @event = event.to_sym
        @fire = :"fire_#{machine}_event"
      end

      def tick(ctx, _now) = @access.call(ctx).public_send(@fire, @event) ? :success : :failure
    end

    # Waits on a machine something else drives: succeeds in a `success` state,
    # fails in a `failure` state, runs otherwise. If the tree abandons it,
    # fires `on_halt` (typically :cancel).
    class WaitFor
      def initialize(access, success:, failure: [], on_halt: nil, machine: :state)
        @access = Machine.accessor(access)
        @success = Array(success).map(&:to_sym).freeze
        @failure = Array(failure).map(&:to_sym).freeze
        @on_halt = on_halt&.to_sym
        @reader = :"#{machine}_name"
        @fire = :"fire_#{machine}_event"
      end

      def tick(ctx, _now)
        state = @access.call(ctx).public_send(@reader)
        return :success if @success.include?(state)
        return :failure if @failure.include?(state)

        :running
      end

      # A rejected event means the machine already left the states that accept
      # it, e.g. a job that completed after the last tick. The tree abandons
      # the leaf either way, so the result is deliberately ignored.
      def halt(ctx)
        @access.call(ctx).public_send(@fire, @on_halt) if @on_halt
        nil
      end
    end
  end
end
