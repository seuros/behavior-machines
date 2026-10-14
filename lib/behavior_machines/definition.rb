# frozen_string_literal: true

module BehaviorMachines
  # Validates a node definition and lowers it to the plain nested-Array spec
  # that both backends build from, so they always agree on shape and on the
  # pre-order numbering observers see.
  module Definition
    # The clock is an unsigned 64-bit count of nanoseconds.
    MAX_NANOS = (2**64) - 1

    module_function

    def compile(node)
      case node
      when Node::Leaf then leaf(node.leaf)
      when Node::AlwaysSuccess then %i[constant success]
      when Node::AlwaysFailure then %i[constant failure]
      when Node::Sleep then [:sleep, nanos(node.seconds, 'Sleep')]
      when Node::Sequence then ordered(node.children, 'Sequence', :success, false)
      when Node::ReactiveSequence then ordered(node.children, 'ReactiveSequence', :success, true)
      when Node::Fallback then ordered(node.children, 'Fallback', :failure, false)
      when Node::ReactiveFallback then ordered(node.children, 'ReactiveFallback', :failure, true)
      when Node::Parallel then parallel(node)
      when Node::IfThenElse then conditional(node.condition, node.then_branch, node.otherwise, false)
      when Node::WhileDoElse then conditional(node.condition, node.body, node.otherwise, true)
      when Node::Inverter then [:map, compile(node.child), :inverter]
      when Node::ForceSuccess then [:map, compile(node.child), :force_success]
      when Node::ForceFailure then [:map, compile(node.child), :force_failure]
      when Node::Timeout then [:timeout, nanos(node.seconds, 'Timeout'), compile(node.child)]
      when Node::Repeat then [:loop, compile(node.child), :success, limit(node.limit, 'Repeat')]
      when Node::RetryUntilSuccessful
        [:loop, compile(node.child), :failure, limit(node.limit, 'RetryUntilSuccessful')]
      when Node::KeepRunningUntilFailure then [:loop, compile(node.child), :success, nil]
      else raise BuildError, "#{node.inspect} is not a BehaviorMachines node"
      end
    end

    def leaf(object)
      raise BuildError, "leaf #{object.inspect} must respond to tick(ctx, now)" unless object.respond_to?(:tick)

      [:leaf, object, object.respond_to?(:halt)]
    end

    def ordered(children, name, advance_on, reactive)
      [:ordered, children_of(children, name), advance_on, reactive]
    end

    def parallel(node)
      children = children_of(node.children, 'Parallel')
      [:parallel, children, threshold(node.success, children.size), threshold(node.failure, children.size)]
    end

    def conditional(condition, branch, otherwise, reactive)
      # Only nil means absent; anything else must be a node.
      [:conditional, compile(condition), compile(branch), otherwise.nil? ? nil : compile(otherwise), reactive]
    end

    def children_of(children, name)
      raise BuildError, "#{name} needs at least one child" if children.empty?

      children.map { |child| compile(child) }
    end

    def threshold(value, children)
      return children if value == :all
      raise BuildError, "Parallel threshold must be :all or an Integer, got #{value.inspect}" unless value.is_a?(Integer)
      raise BuildError, "Parallel threshold #{value} must be between 1 and #{children}" unless value.between?(1, children)

      value
    end

    def nanos(seconds, name)
      unless seconds.is_a?(Numeric) && seconds.real? && seconds.finite? && !seconds.negative?
        raise BuildError, "#{name} needs a finite, non-negative number of seconds, got #{seconds.inspect}"
      end

      nanos = (seconds.to_r * 1_000_000_000).round
      raise BuildError, "#{name} of #{seconds} seconds exceeds the 64-bit nanosecond clock" if nanos > MAX_NANOS

      nanos
    end

    def limit(value, name)
      return nil if value == :unlimited
      return value if value.is_a?(Integer) && value.between?(0, MAX_NANOS)

      raise BuildError, "#{name} limit must be :unlimited or an Integer in 0..2**64-1, got #{value.inspect}"
    end
  end
end
