# frozen_string_literal: true

module BehaviorMachines
  # Immutable node definitions. Build them with the module functions on
  # BehaviorMachines (BehaviorMachines.sequence, .parallel, ...) and the
  # decorator methods every node responds to, then compile with Tree.new.
  #
  # Names and semantics follow the behavior-tree pattern, matching the
  # behavior-machines Rust crate.
  module Node
    # Decorators available on every node.
    module Decorators
      def inverter = Inverter.new(child: self)
      def force_success = ForceSuccess.new(child: self)
      def force_failure = ForceFailure.new(child: self)
      def timeout(seconds) = Timeout.new(seconds:, child: self)
      def repeat(limit) = Repeat.new(limit:, child: self)
      def retry_until_successful(limit) = RetryUntilSuccessful.new(limit:, child: self)
      def keep_running_until_failure = KeepRunningUntilFailure.new(child: self)
    end

    def self.define(*members) = Data.define(*members) { include Decorators }
    private_class_method :define

    Leaf = define(:leaf)
    AlwaysSuccess = define
    AlwaysFailure = define
    Sleep = define(:seconds)
    Sequence = define(:children)
    ReactiveSequence = define(:children)
    Fallback = define(:children)
    ReactiveFallback = define(:children)
    Parallel = define(:children, :success, :failure)
    IfThenElse = define(:condition, :then_branch, :otherwise)
    WhileDoElse = define(:condition, :body, :otherwise)
    Inverter = define(:child)
    ForceSuccess = define(:child)
    ForceFailure = define(:child)
    Timeout = define(:seconds, :child)
    Repeat = define(:limit, :child)
    RetryUntilSuccessful = define(:limit, :child)
    KeepRunningUntilFailure = define(:child)
  end

  class << self
    # A condition check or action: any object responding to tick(ctx, now)
    # and, optionally, halt(ctx).
    def leaf(leaf) = Node::Leaf.new(leaf:)

    # Succeeds immediately.
    def always_success = Node::AlwaysSuccess.new

    # Fails immediately.
    def always_failure = Node::AlwaysFailure.new

    # Runs until `seconds` have passed since it started, then succeeds.
    def sleep(seconds) = Node::Sleep.new(seconds:)

    # Ticks children in order; fails on the first failure, succeeds once all
    # succeed. A running child is resumed on the next tick.
    def sequence(*children) = Node::Sequence.new(children: children.freeze)

    # A sequence that restarts from its first child every tick, halting a
    # running later child once an earlier one stops succeeding.
    def reactive_sequence(*children) = Node::ReactiveSequence.new(children: children.freeze)

    # Ticks children in order; succeeds on the first success, fails once all
    # fail. A running child is resumed on the next tick.
    def fallback(*children) = Node::Fallback.new(children: children.freeze)

    # A fallback that restarts from its first child every tick, so a
    # higher-priority child preempts (halts) a running lower-priority one.
    def reactive_fallback(*children) = Node::ReactiveFallback.new(children: children.freeze)

    # Ticks every unfinished child each tick. Succeeds once `success` children
    # succeeded; fails once `failure` failed or success became unreachable.
    # Thresholds are :all or an Integer; by default every child must succeed
    # and one failure fails the node.
    def parallel(*children, success: :all, failure: 1)
      Node::Parallel.new(children: children.freeze, success:, failure:)
    end

    # Ticks `condition` until it finishes, then runs `then_branch` or
    # `otherwise`. Without `otherwise`, a failed condition fails the node.
    def if_then_else(condition, then_branch, otherwise = nil)
      Node::IfThenElse.new(condition:, then_branch:, otherwise:)
    end

    # The reactive if_then_else: re-checks `condition` every tick, pausing the
    # branches while it runs and halting the unselected branch when it flips.
    def while_do_else(condition, body, otherwise = nil)
      Node::WhileDoElse.new(condition:, body:, otherwise:)
    end
  end
end
