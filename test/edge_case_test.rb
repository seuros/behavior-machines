# frozen_string_literal: true

require_relative 'test_helper'
require 'weakref'
require 'state_machines'

# Error paths, timer edges, GC retention, and state_machines integration.
# Runs against both backends (rake test:both).
class EdgeCaseTest < Minitest::Test
  class ProbeError < StandardError; end
  class FatalProbe < Exception; end

  class Tripwire
    attr_accessor :armed

    def tick(_ctx, _now)
      raise ProbeError, 'tick failed' if armed

      :running
    end
  end

  class Observer
    attr_accessor :trip_tick, :trip_halt
    attr_reader :reports

    def initialize = @reports = []

    def ticked(id, status)
      @reports << [:ticked, id, status]
      raise ProbeError, 'ticked failed' if id == trip_tick
    end

    def halted(id)
      @reports << [:halted, id]
      raise ProbeError, 'halted failed' if id == trip_halt
    end
  end

  def test_error_in_parallel_any_does_not_erase_active_sibling
    script = Script.new
    bomb = Tripwire.new
    subject = tree(parallel_any(BM.leaf(bomb), leaf('active')))
    assert_equal :running, subject.tick(script, now: 0)
    bomb.armed = true
    assert_raises(ProbeError) { subject.tick(script, now: 1) }
    assert_equal 1, script.count(halted('active')), script.log.inspect
    script.assert_idle!
    subject.halt(script)
    assert_equal 1, script.count(halted('active'))
    bomb.armed = false
    assert_equal :running, subject.tick(script, now: 2)
    subject.halt(script)
    script.assert_idle!
  end

  def test_invalid_status_in_reactive_fallback_preserves_sibling_for_cleanup
    script = Script.new
    script.set('guard', :failure)
    subject = tree(BM.reactive_fallback(leaf('guard'), leaf('active')))
    assert_equal :running, subject.tick(script, now: 0)
    script.set('guard', :invalid)
    assert_raises(BM::InvalidStatusError) { subject.tick(script, now: 1) }
    assert_equal 1, script.count(halted('active')), script.log.inspect
    script.assert_idle!
    subject.halt(script)
    assert_equal 1, script.count(halted('active'))
  end

  def test_ticked_observer_error_preserves_unticked_active_sibling_for_cleanup
    script = Script.new
    observer = Observer.new
    subject = tree(parallel_any(leaf('left'), leaf('right')))
    assert_equal :running, subject.tick(script, now: 0, observer:)
    observer.reports.clear
    observer.trip_tick = 1
    assert_raises(ProbeError) { subject.tick(script, now: 1, observer:) }
    assert_equal [[:ticked, 1, :running]], observer.reports
    assert_equal [1, 1], %w[left right].map { script.count(halted(_1)) }
    script.assert_idle!
  end

  def test_halted_observer_error_still_cleans_up_every_active_leaf
    script = Script.new
    observer = Observer.new
    subject = tree(BM.parallel(leaf('a'), leaf('b'), leaf('c')))
    subject.tick(script, now: 0)
    observer.trip_halt = 1
    assert_raises(ProbeError) { subject.halt(script, observer:) }
    assert_equal [[:halted, 1]], observer.reports
    assert_equal [1, 1, 1], %w[a b c].map { script.count(halted(_1)) }
    script.assert_idle!
    subject.halt(script)
    assert_equal [1, 1, 1], %w[a b c].map { script.count(halted(_1)) }
  end

  def test_halted_observer_error_inside_tick_does_not_hide_nested_active_leaves
    script = Script.new
    observer = Observer.new
    script.set('guard', :success)
    # ids: root 0, guard 1, parallel 2, a 3, b 4.
    subject = tree(BM.reactive_sequence(leaf('guard'), BM.parallel(leaf('a'), leaf('b'))))
    assert_equal :running, subject.tick(script, now: 0)
    script.set('guard', :failure)
    observer.trip_halt = 3
    assert_raises(ProbeError) { subject.tick(script, now: 1, observer:) }
    assert_equal [[:ticked, 1, :failure], [:halted, 3]], observer.reports
    assert_equal [1, 1], %w[a b].map { script.count(halted(_1)) }
    script.assert_idle!
    subject.halt(script)
    assert_equal [1, 1], %w[a b].map { script.count(halted(_1)) }
  end

  def test_limits_beyond_the_64_bit_range_are_rejected_at_build_time
    [leaf('work').repeat(2**64), leaf('work').retry_until_successful(2**64)].each do |root|
      assert_raises(BM::BuildError) { tree(root) }
    end
    assert_equal :running, tree(leaf('work').repeat((2**64) - 1)).tick(Script.new, now: 0)
  end

  def test_false_otherwise_is_rejected_by_shared_definition_validation
    # nil means absent; false is not a node. It must not be silently treated
    # as absent by one backend and sent to an Array conversion by the other.
    root = BM.if_then_else(BM.always_failure, BM.always_success, false)
    assert_raises(BM::BuildError) { tree(root) }
  end

  class InspectRaises
    def tick(_ctx, _now) = :invalid
    def inspect = raise(ProbeError, 'inspect failed')
  end

  def test_inspect_exception_propagates_identically_when_reporting_invalid_status
    script = Script.new
    subject = tree(BM.parallel(leaf('active'), BM.leaf(InspectRaises.new)))
    error = assert_raises(ProbeError) { subject.tick(script, now: 0) }
    assert_equal 'inspect failed', error.message
    assert_equal 1, script.count(halted('active'))
    script.assert_idle!
  end

  def test_fatal_exception_cleans_up_without_observer_reports
    script = Script.new
    observer = Observer.new
    bomb = Object.new
    def bomb.tick(_ctx, _now) = raise(FatalProbe, 'fatal')
    subject = tree(BM.parallel(leaf('active'), BM.leaf(bomb)))
    assert_raises(FatalProbe) { subject.tick(script, now: 0, observer:) }
    assert_equal [[:ticked, 1, :running]], observer.reports
    assert_equal [tick('active'), halted('active')], script.log
    script.assert_idle!
    refute subject.running?
  end

  def test_root_observer_exception_cleans_up_running_tree
    script = Script.new
    observer = Observer.new
    observer.trip_tick = 0
    subject = tree(BM.parallel(leaf('a'), leaf('b')))
    assert_raises(ProbeError) { subject.tick(script, now: 0, observer:) }
    assert_equal [1, 1], %w[a b].map { script.count(halted(_1)) }
    refute observer.reports.any? { _1.first == :halted }
    refute subject.running?
    script.assert_idle!
  end

  def test_timers_round_exactly_and_overflow_does_not_fire
    # Rational avoids floating-point loss near 2**64.
    script = Script.new
    max = (2**64) - 1
    subject = tree(BM.sleep(Rational(max, 1_000_000_000)))
    assert_equal :running, subject.tick(script, now: 0)
    assert_equal max, subject.next_deadline
    assert_equal :success, subject.tick(script, now: max)
    assert_nil subject.next_deadline

    subject = tree(leaf('work').timeout(Rational(2, 1_000_000_000)))
    assert_equal :running, subject.tick(script, now: max - 1)
    assert_nil subject.next_deadline
    assert_equal :running, subject.tick(script, now: max)
    subject.halt(script)
    script.assert_idle!

    subject = tree(BM.sleep(Rational(1, 2_000_000_000)))
    assert_equal :running, subject.tick(script, now: 0)
    assert_equal 1, subject.next_deadline # Exactly half a nanosecond rounds up.
    assert_equal :success, subject.tick(script, now: 1)
    assert_raises(BM::BuildError) { tree(BM.sleep(Rational(max + 1, 1_000_000_000))) }
  end

  def test_definition_copies_children_but_keeps_mutable_leaf_objects
    script = Script.new
    children = [leaf('a'), leaf('b')]
    subject = tree(BM::Node::Sequence.new(children:))
    children.clear
    script.set('a', :success)
    assert_equal :running, subject.tick(script, now: 0)
    assert_equal [tick('a'), tick('b')], script.log
    subject.halt(script)
    script.assert_idle!
  end

  def test_reentrant_calls_raise_without_borrow_panics
    %i[tick halt running? next_deadline].each do |operation|
      leaf_object = Object.new
      leaf_object.define_singleton_method(:tick) do |ctx, _now|
        operation == :tick ? ctx.tick(ctx, now: 0) : ctx.public_send(operation)
      end
      subject = tree(BM.leaf(leaf_object))
      assert_raises(BM::ReentrantCallError) { subject.tick(subject, now: 0) }
      refute subject.running?
    end
  end

  class GcLeaf
    def tick(script, _now)
      GC.start
      GC.compact if GC.respond_to?(:compact) # not on every Ruby engine
      script.tick_leaf('gc')
    end

    def halt(script)
      GC.start
      script.halt_leaf('gc')
    end
  end

  def gc_tree
    object = GcLeaf.new
    [tree(BM.leaf(object)), WeakRef.new(object)]
  end

  def test_native_mark_keeps_leaf_alive_during_tick_halt_and_compaction
    # Bounded collections only, deliberately not GC.stress.
    subject, weak = gc_tree
    GC.start
    assert weak.weakref_alive?
    script = Script.new
    observer = Observer.new
    assert_equal :running, subject.tick(script, now: 0, observer:)
    subject.halt(script, observer:)
    assert_equal [[:ticked, 0, :running], [:halted, 0]], observer.reports
    script.assert_idle!
    assert weak.weakref_alive?
  end

  class GcHalt
    def tick(script, _now) = script.tick_leaf('active')

    def halt(script)
      GC.start
      GC.compact if GC.respond_to?(:compact) # not on every Ruby engine
      script.halt_leaf('active')
    end
  end

  def test_exception_survives_gc_during_native_cleanup
    script = Script.new
    bomb = Object.new
    def bomb.tick(_ctx, _now) = raise(ProbeError, 'fresh exception payload')
    subject = tree(BM.parallel(BM.leaf(GcHalt.new), BM.leaf(bomb)))
    error = assert_raises(ProbeError) { subject.tick(script, now: 0) }
    assert_equal 'fresh exception payload', error.message
    script.assert_idle!
  end

  class Job
    state_machine :job, initial: :pending do
      event(:start) { transition pending: :started }
      event(:complete) { transition started: :completed }
      event(:cancel) { transition %i[pending started] => :cancelled }
    end
  end

  def test_real_named_state_machine_helpers_and_cancellation
    job = Job.new
    access = ->(ctx) { ctx }
    fire = BM::Machine::Fire.new(access, :start, machine: :job)
    assert_equal :success, fire.tick(job, 0)
    assert_equal :failure, fire.tick(job, 0)
    condition = BM::Machine::InState.new(access, :started, machine: :job)
    assert_equal :success, condition.tick(job, 0)
    waiter = BM::Machine::WaitFor.new(
      access, success: :completed, failure: :cancelled, on_halt: :cancel, machine: :job
    )
    subject = tree(BM.leaf(waiter))
    assert_equal :running, subject.tick(job, now: 0)
    subject.halt(job)
    subject.halt(job)
    assert_equal :cancelled, job.job_name
    assert_equal :failure, waiter.tick(job, 0)
  end
end
