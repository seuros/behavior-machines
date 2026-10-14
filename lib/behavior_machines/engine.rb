# frozen_string_literal: true

module BehaviorMachines
  # The pure-Ruby backend. It mirrors the Rust engine node for node; the same
  # test suite runs against both backends.
  module Engine
    MAX_TIME = Definition::MAX_NANOS

    # Discards observer reports.
    module NullObserver
      def self.ticked(_id, _status) = nil
      def self.halted(_id) = nil
    end

    # One tick or halt. `errors` collects what leaves and observers raise; once
    # it holds one, no more leaf ticks or observer reports run this call.
    Run = Data.define(:ctx, :observer, :errors) do
      def self.start(ctx, observer) = new(ctx, observer || NullObserver, [])

      def failed? = !errors.empty?

      def record(error)
        errors << error
        nil
      end

      def report_ticked(id, status) = report { observer.ticked(id, status) }
      def report_halted(id) = report { observer.halted(id) }

      private

      def report
        return if failed?

        yield
      rescue Exception => e # rubocop:disable Lint/RescueException -- recorded, raised after cleanup
        record(e)
      end
    end

    # A deadline past the end of the clock never fires.
    def self.deadline_after(now, nanos)
      deadline = now + nanos
      deadline > MAX_TIME ? nil : deadline
    end

    def self.reached?(deadline, now) = !deadline.nil? && now >= deadline

    # Compiled tree plus its execution state.
    class Tree
      def initialize(spec)
        @root = Builder.new.build(spec)
      end

      # Once a leaf or observer raises, the rest of the tick runs no Ruby: an
      # active leaf stays active and an idle one does not start. Then every
      # leaf still active is halted, without observer reports, and the first
      # error propagates. Errors raised during that cleanup are dropped.
      def tick(ctx, now, observer)
        run = Run.start(ctx, observer)
        status = @root.tick(run, now)
        return status unless run.failed?

        @root.halt(Run.start(ctx, nil))
        raise run.errors.first
      end

      # Every active leaf is halted even if one, or the observer, raises; the
      # first error propagates afterwards.
      def halt(ctx, observer)
        run = Run.start(ctx, observer)
        @root.halt(run)
        raise run.errors.first if run.failed?
      end

      def running? = @root.running?
      def next_deadline = @root.next_deadline
    end

    # Builds slots from a spec, numbering nodes in pre-order.
    class Builder
      def initialize
        @next_id = 0
      end

      def build(spec)
        id = @next_id
        @next_id += 1
        kind, *args = spec
        case kind
        when :leaf then LeafSlot.new(id, *args)
        when :constant then ConstantSlot.new(id, *args)
        when :sleep then SleepSlot.new(id, *args)
        when :ordered then ordered(id, *args)
        when :parallel then parallel(id, *args)
        when :conditional then conditional(id, *args)
        when :map then MapSlot.new(id, build(args[0]), args[1])
        when :timeout then TimeoutSlot.new(id, args[0], build(args[1]))
        when :loop then LoopSlot.new(id, build(args[0]), args[1], args[2])
        else raise ArgumentError, "unknown spec #{kind.inspect}"
        end
      end

      private

      def ordered(id, children, advance_on, reactive)
        (reactive ? ReactiveSlot : OrderedSlot).new(id, children.map { |child| build(child) }, advance_on)
      end

      def parallel(id, children, success, failure)
        ParallelSlot.new(id, children.map { |child| build(child) }, success, failure)
      end

      def conditional(id, condition, branch, otherwise, reactive)
        condition = build(condition)
        branch = build(branch)
        otherwise &&= build(otherwise)
        (reactive ? WhileDoElseSlot : IfThenElseSlot).new(id, condition, branch, otherwise)
      end
    end

    # One compiled node. `@running` is set while its last tick returned
    # :running and it has not been halted since; only running slots halt.
    class Slot
      def initialize(id)
        @id = id
        @running = false
      end

      def running? = @running

      def tick(run, now)
        status = step(run, now, !@running)
        @running = status == :running
        run.report_ticked(@id, status)
        status
      end

      def halt(run)
        return unless @running

        @running = false
        stop(run)
        run.report_halted(@id)
      end

      def next_deadline = @running ? running_deadline : nil

      private

      def stop(_run) = nil
      def running_deadline = nil
      def halt_all(slots, run) = slots.each { |slot| slot.halt(run) }
      def earliest(deadlines) = deadlines.compact.min
    end

    # A user leaf.
    class LeafSlot < Slot
      def initialize(id, leaf, halts)
        super(id)
        @leaf = leaf
        @halts = halts
      end

      private

      # After an error no more leaf code runs this call: an active leaf stays
      # active (so cleanup halts it) and an idle one does not start. A leaf
      # that raises counts as finished.
      def step(run, now, starting)
        return starting ? :failure : :running if run.failed?

        BehaviorMachines.check_status(@leaf.tick(run.ctx, now), @leaf)
      rescue Exception => e # rubocop:disable Lint/RescueException -- recorded, raised after cleanup
        run.record(e)
        :failure
      end

      def stop(run)
        @leaf.halt(run.ctx) if @halts
      rescue Exception => e # rubocop:disable Lint/RescueException -- recorded; the halt continues
        run.record(e)
      end
    end

    # AlwaysSuccess and AlwaysFailure.
    class ConstantSlot < Slot
      def initialize(id, status)
        super(id)
        @status = status
      end

      private

      def step(_run, _now, _starting) = @status
    end

    class SleepSlot < Slot
      def initialize(id, nanos)
        super(id)
        @nanos = nanos
        @deadline = nil
      end

      private

      def step(_run, now, starting)
        @deadline = Engine.deadline_after(now, @nanos) if starting
        Engine.reached?(@deadline, now) ? :success : :running
      end

      def running_deadline = @deadline
    end

    # Sequence and Fallback with memory: resume the running child.
    class OrderedSlot < Slot
      def initialize(id, children, advance_on)
        super(id)
        @children = children
        @advance_on = advance_on
        @cursor = 0
      end

      private

      def step(run, now, _starting)
        while (child = @children[@cursor])
          status = child.tick(run, now)
          return :running if status == :running
          return finish(status) unless status == @advance_on

          @cursor += 1
        end
        finish(@advance_on)
      end

      def finish(status)
        @cursor = 0
        status
      end

      def stop(run)
        halt_all(@children, run)
        @cursor = 0
      end

      def running_deadline = earliest(@children.map(&:next_deadline))
    end

    # ReactiveSequence and ReactiveFallback: re-walk from the first child.
    class ReactiveSlot < Slot
      def initialize(id, children, advance_on)
        super(id)
        @children = children
        @advance_on = advance_on
      end

      private

      def step(run, now, _starting)
        @children.each_with_index do |child, index|
          status = child.tick(run, now)
          next if status == @advance_on

          halt_all(@children.drop(index + 1), run)
          return status
        end
        @advance_on
      end

      def stop(run) = halt_all(@children, run)
      def running_deadline = earliest(@children.map(&:next_deadline))
    end

    class ParallelSlot < Slot
      def initialize(id, children, success, failure)
        super(id)
        @children = children
        @success = success
        @failure = failure
        @outcomes = Array.new(children.size)
      end

      private

      def step(run, now, _starting)
        decided = decide(run, now)
        return :running unless decided

        halt_all(@children, run)
        @outcomes.fill(nil)
        decided
      end

      # Children decide in order, so the leftmost wins ties.
      def decide(run, now)
        succeeded = @outcomes.count(:success)
        failed = @outcomes.count(:failure)
        @children.each_with_index do |child, index|
          next if @outcomes[index]

          status = child.tick(run, now)
          next if status == :running

          @outcomes[index] = status
          status == :success ? succeeded += 1 : failed += 1
          return :success if succeeded >= @success
          return :failure if failed >= @failure || @children.size - failed < @success
        end
        nil
      end

      def stop(run)
        halt_all(@children, run)
        @outcomes.fill(nil)
      end

      def running_deadline = earliest(@children.map(&:next_deadline))
    end

    # Shared by IfThenElse and WhileDoElse.
    class ConditionalSlot < Slot
      def initialize(id, condition, branch, otherwise)
        super(id)
        @condition = condition
        @branch = branch
        @otherwise = otherwise
      end

      private

      def stop(run)
        [@condition, @branch, @otherwise].compact.each { |slot| slot.halt(run) }
      end

      def running_deadline = earliest([@condition, @branch, @otherwise].compact.map(&:next_deadline))
    end

    class IfThenElseSlot < ConditionalSlot
      def initialize(...)
        super
        @selected = nil
      end

      private

      def step(run, now, _starting)
        unless @selected
          case @condition.tick(run, now)
          when :running then return :running
          when :success then @selected = :branch
          else @selected = :otherwise
          end
        end
        chosen = @selected == :branch ? @branch : @otherwise
        return finish(:failure) unless chosen

        status = chosen.tick(run, now)
        status == :running ? status : finish(status)
      end

      def finish(status)
        @selected = nil
        status
      end

      def stop(run)
        super
        @selected = nil
      end
    end

    # While the condition runs the branches are paused: neither ticked nor
    # halted. Otherwise the unselected branch is halted before the other ticks.
    class WhileDoElseSlot < ConditionalSlot
      private

      def step(run, now, _starting)
        case @condition.tick(run, now)
        when :running then :running
        when :success
          @otherwise&.halt(run)
          @branch.tick(run, now)
        else
          @branch.halt(run)
          @otherwise ? @otherwise.tick(run, now) : :failure
        end
      end

      # A paused branch's timers cannot act until the condition finishes.
      def running_deadline = @condition.running? ? @condition.next_deadline : super
    end

    # Inverter, ForceSuccess, and ForceFailure.
    class MapSlot < Slot
      def initialize(id, child, map)
        super(id)
        @child = child
        @map = map
      end

      private

      def step(run, now, _starting)
        status = @child.tick(run, now)
        return status if status == :running

        case @map
        when :inverter then status == :success ? :failure : :success
        when :force_success then :success
        else :failure
        end
      end

      def stop(run) = @child.halt(run)
      def running_deadline = @child.next_deadline
    end

    # The child only ticks strictly before the deadline.
    class TimeoutSlot < Slot
      def initialize(id, nanos, child)
        super(id)
        @nanos = nanos
        @child = child
        @deadline = nil
      end

      private

      def step(run, now, starting)
        @deadline = Engine.deadline_after(now, @nanos) if starting
        if Engine.reached?(@deadline, now)
          @child.halt(run)
          return :failure
        end
        @child.tick(run, now)
      end

      def stop(run) = @child.halt(run)
      def running_deadline = earliest([@deadline, @child.next_deadline])
    end

    # Repeat, RetryUntilSuccessful, and KeepRunningUntilFailure: at most one
    # finished child run per tick.
    class LoopSlot < Slot
      def initialize(id, child, again_on, limit)
        super(id)
        @child = child
        @again_on = again_on
        @limit = limit
        @runs = 0
      end

      private

      def step(run, now, _starting)
        return @again_on if @limit&.zero?

        status = @child.tick(run, now)
        return status if status == :running
        return finish(status) unless status == @again_on
        return :running if @limit.nil?
        return finish(status) if @runs + 1 == @limit

        @runs += 1
        :running
      end

      def finish(status)
        @runs = 0
        status
      end

      def stop(run)
        @child.halt(run)
        @runs = 0
      end

      def running_deadline = @child.next_deadline
    end
  end
end
