# frozen_string_literal: true

require_relative 'test_helper'

class DefinitionTest < Minitest::Test
  def test_rejects_empty_composites_at_any_depth
    cases = {
      BM.sequence => 'Sequence',
      BM.reactive_sequence => 'ReactiveSequence',
      BM.fallback => 'Fallback',
      BM.reactive_fallback => 'ReactiveFallback',
      BM.parallel => 'Parallel',
      BM.sequence(leaf('a'), BM.fallback.inverter) => 'Fallback'
    }
    cases.each do |root, name|
      error = assert_raises(BM::BuildError) { tree(root) }
      assert_equal "#{name} needs at least one child", error.message
    end
  end

  def test_rejects_parallel_thresholds_outside_the_child_count
    [[0, :all], [:all, 3], [:some, 1]].each do |success, failure|
      assert_raises(BM::BuildError, [success, failure].inspect) do
        tree(BM.parallel(leaf('a'), leaf('b'), success:, failure:))
      end
    end
    error = assert_raises(BM::BuildError) { tree(BM.parallel(leaf('a'), leaf('b'), success: 0)) }
    assert_equal 'Parallel threshold 0 must be between 1 and 2', error.message
  end

  def test_rejects_invalid_durations_limits_and_leaves
    invalid = [
      BM.sleep(-1), BM.sleep(Float::INFINITY), BM.sleep('1'), BM.sleep(2**64),
      leaf('a').timeout(-0.5), leaf('a').repeat(-1), leaf('a').retry_until_successful(:forever),
      BM.leaf(Object.new), BM.sequence(:not_a_node)
    ]
    invalid.each { |root| assert_raises(BM::BuildError, root.inspect) { tree(root) } }
  end

  def test_definitions_are_immutable_values
    node = BM.sequence(leaf('a'), leaf('b'))
    assert_predicate node, :frozen?
    assert_predicate node.children, :frozen?
    assert_equal BM.sleep(1), BM.sleep(1)
  end

  def test_rejects_unknown_backends
    assert_raises(ArgumentError) { BM::Tree.new(leaf('a'), backend: :gpu) }
    return if BM.native_available?

    assert_raises(BM::Error) { BM::Tree.new(leaf('a'), backend: :native) }
  end
end
